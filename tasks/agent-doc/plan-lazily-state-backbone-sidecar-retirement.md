# Plan: Replace hot-path sidecars with the lazily state backbone

`#sidecarstate`

## Operator ask

During an `agent-doc tasks/professional/equityfundingsource.md` run on
2026-07-04, preflight exposed this failure directly to the agent:

> editor is the current authority for the document, but the CRDT relay has no
> registered replica yet; disk is a non-authoritative replica and was not read

The expected behavior is that agent-doc auto-starts or reconciles the document
model before preflight returns. The agent should not see that error class as the
normal prompt contract.

Follow-up operator direction: sidecars should be replaced with the lazily state
backbone.

## Failure class

The incident combined two related problems:

1. The document authority classifier knew a live editor was authoritative, but
   the CRDT relay had no registered replica and did not auto-start/reconcile it.
2. Recovery then depended on snapshot/cycle sidecars. The visible document had
   moved forward, so the retained `response_captured` state and baseline drift
   blocked preflight/write recovery instead of deriving current truth from the
   live document model.

This is not just a better error message request. It is a state model problem:
sidecars are being asked to carry hot-path authority that should live in a
typed, reactive document state backbone.

## Desired model

Use the lazily-backed state backbone as the source for document/session state:

- The document model auto-starts on demand when preflight, sync, finalize, or
  session-check needs a live authoritative view.
- When the file is open in a maintained editor, the editor buffer / realtime
  document model is the text source of truth. The on-disk file is only the
  detached fallback replica chosen by the realtime resolver; hot-path code should
  not read disk directly to decide visible/current document state.
- Document-visible mutations should target the lazily document-cell tree/current
  document model. Backlog edits, queue edits, exchange append/replay, boundary
  movement, prompt prefixing, closeout recovery restores, pending reap, template
  changes, and frontmatter changes are document-cell transactions first; markdown
  text and disk content are projections/flushes selected by authority. The
  canonical Rust surface is `agent_doc_merge::document_cell::{DocumentCellTree,
  DocumentCellMutation}`. Whole-document strings are only import/export
  projections at legacy boundaries; new cycle code should express focused
  `Set`/`Insert`/`Remove`/`Move` cell mutations so unrelated components cannot
  cross over.
- Turn snapshots remain relevant only as immutable baselines/checkpoints for
  comparison. They are not competing document sources. Once all current text
  flows through the realtime model, "drift" means a named baseline comparison
  result, not disagreement about which source owns the document.
- `agent-doc-flow-io/src/closeout.rs` still has path-heavy function signatures
  because the old closeout pipeline treated `file: &Path` as both identity and
  content authority. Keep `DocumentKey` / `CurrentDocument` at the outer
  closeout boundary, pass current-document/content handles through helpers that
  operate on visible text, and only derive `doc.key().as_path()` at integration
  edges that truly key git, snapshots, captures, logs, IPC patch cleanup, or
  project-root lookup by path.
- Cutover progress: `DocumentKey` and `CurrentDocument` now live in the pure
  `agent-doc-document-realtime` crate and are re-exported by
  `agent-doc-document-realtime-io`. `CloseoutEffects::resolve_current_document`
  returns `CurrentDocument`, and closeout visible recovery writes pass that
  handle into an authority-checked write instead of writing by path alone.
- Cutover progress: the per-cell lazily model has been renamed from
  `agent_doc_merge::cell_doc` to `agent_doc_merge::document_cell`, and the public
  tree/mutation types are now `DocumentCellTree` / `DocumentCellMutation`.
  `CurrentDocument` can project its authoritative content to a `DocumentCellTree`
  and apply a focused `DocumentCellMutation` without inventing a duplicate tree
  wrapper.
- Context-schema decision: lazily now has typed context-family primitives so
  `Slot`/`Cell` handles can be bound to a schema instead of being accidentally
  reused across parallel graphs. Agent-doc should use direct typed lazily
  contexts, not wrapper structs: the short-lived cycle IO graph is
  `TypedContext<CycleContextSchema>` via the `CycleContext` alias, long-lived
  actor state is `TypedContext<ActorContextSchema>` via the `ActorContext`
  alias, and any future realtime document graph should be a typed lazily
  `ThreadSafeContext` schema rather than `DocumentContext` /
  `RealtimeDocumentContext` wrappers.
- Cutover progress: `CycleContext` and `ActorContext` now alias direct
  `lazily::TypedContext` schemas, with memoized Slot/Cell factories living in
  `agent-doc-run-context-io`. The old `DocumentContext` wrapper and the
  temporary `RunContext` compatibility alias/factory are removed; callers use
  `CycleContext` / `cycle_context(...)` plus `AgentDocContextExt` while the
  remaining realtime document graph is still being collapsed into direct lazily
  contexts.
- Realtime steering is one such baseline comparison. If the operator edits the
  prompt during an in-flight turn, the current document model is compared with
  the turn baseline and projected as `prompt_target`, `content_edit`,
  `prompt_deleted`, or `prompt_reduced`. The first-party plugins must surface
  this steering state on the active agent-doc turn banner/status label so a
  deleted/reduced prompt is visible before the old response can be persisted.
- The state backbone owns the current facts: editor attachment, CRDT replica
  registration, editor-sync status, lifecycle phase, capture/recovery state,
  commit barrier readiness, and supervisor staleness.
- Every pipeline stage is reentrant. If the process crashes after any durable
  transition, restarting the same stage must either complete the transition,
  prove it already completed, or reconcile the drift into a named backbone state
  before continuing.
- Snapshot, legacy ack-content, live-buffer, and cycle-state sidecars become
  derived audit/recovery projections. A stale/missing sidecar can warn or assist
  repair, but it cannot be the reason preflight exposes an unreconciled authority
  error when the model can be started or rebuilt.
- Transport receipts are bidirectional lazily state, not ACK sidecars: binary
  write-intent / expected response facts flow toward the editor, and editor
  patch-received/applied/rejected / visible-buffer hash facts flow back. All
  maintained editor plugins must publish lazily receipt events and use those
  events as the closeout proof source. A plugin that can only publish legacy ACK
  facts is incompatible and must surface a version/install error instead of
  seeding a compatibility projection.
- Commit/finalize still fail closed when operator-visible text authority is
  unproven. The change is that the proof should come from named backbone state
  and state-vector/editor-sync facts, not stale filesystem projections.

This should connect to the existing architecture tracks rather than create a
parallel system:

- `#crdtauth6`: live editor/CRDT authority proof.
- `#adstatechart2` and `#adstatechart3`: named lazily state chart observation
  and load-bearing closeout guards.
- `#cdtcutover`: evented document model queries for preflight/supervisor logic.
- `agent-doc-state-backbone`: the crate that should own the typed state facts
  instead of ad hoc sidecar reads.

## Current migration ledger

Status as of 2026-07-05 in `src/agent-doc`:

- Supervisor recycle in-flight and recycle-yield gates now use
  state-backbone/project-controller projections instead of recycle marker files.
- Route-submit in-flight/blocked state now uses `RouteSubmitStarted`,
  `RouteSubmitSettled`, and `RouteSubmitBlocked` facts instead of
  route-submit marker files.
- Queue context-clear state now uses `QueueContextClearDeferred`,
  `QueueContextClearStarted`, and `QueueContextClearSettled` facts for
  in-flight clears, deferred operator clears, and manual clear-cooldown state;
  `.agent-doc/context-clear-in-flight`, `.agent-doc/deferred-clears`, and
  `.agent-doc/queue-cooldowns` are no longer hot-path authorities.
- Queue drain-stall continuation-pending state now uses
  `QueueDrainStallContinuationRecorded` / `QueueDrainStallContinuationCleared`
  facts instead of `.agent-doc/drain-stall/*` marker files.
- Phase 1 has an initial bounded document-model ensure hook:
  `agent-doc-crdt-relay-io::ensure_document_model(file, source)` asks the live
  editor to republish/register through read-only `publish_live_buffer`, waits
  briefly for the relay model to become usable, and makes
  `try_resolve_current_doc_from_file` report a named
  startup/reconciliation failure instead of the raw "no registered replica"
  contract when the relay cannot be recovered.
- Cycle-state lifecycle transitions now append durable closeout facts to the
  state backbone: `PreflightStarted`, `ResponseCaptured`, `WriteApplied`,
  `CommitObserved`, and `CycleAbandoned`. The append uses stable event ids so a
  crash/retry can replay the same transition idempotently. The
  `.agent-doc/state/cycles/<doc-hash>.json` sidecar remains as a compatibility
  recovery projection during this phase, but a transition no longer reports
  success if the matching closeout fact cannot be recorded.
- Git closeout now appends exact `HEAD` SHA `CommitObserved` facts on both
  successful real commits and already-current no-op closeouts. This supersedes
  the cycle-state bridge's temporary content-hash commit observation in the
  closeout projection whenever git proof is available.
- `agent-doc-cycle-state-io::load_closeout_projection` now replays the durable
  closeout projection from the state backbone. `session-check` uses that
  projection before the JSON cycle sidecar for the open/terminal phase decision:
  a matching-cycle lazily phase overrides stale JSON, and a projection-only open
  cycle fails closed. The JSON sidecar still supplies detailed compatibility
  guard payloads until those facts move into backbone state.
- `agent-doc-cycle-state-io::load_with_closeout_projection` overlays matching
  closeout projection phase/capture metadata onto the JSON compatibility state.
  Preflight's interrupted-cycle guard, early closeout-drift gate, and route queue
  snapshot recovery now use that projection-aware view before deciding whether a
  prior cycle is open or terminal. A projection-only open cycle, including one
  hidden behind missing or mismatched-stale JSON recovery state, fails closed
  before preflight diffs.
- Preflight completion and write-entry phase gates now use the projection-aware
  cycle view for recovery decisions: interrupted-cycle completion, post-repair
  still-open checks, pipeline-state output, route-queue snapshot boundary
  recovery, and bare-write response-body commit escalation no longer let stale
  open JSON override a terminal lazily closeout projection.
- Supervisor/Stop-hook phase gates now use the projection-aware cycle view for
  Codex unchanged-file open-cycle detection, route-owned supervisor baselines,
  mid-turn recycle resume checkpoints, idle-watch convergence facts, blocked
  convergence playback metadata, and supervisor self-recycle open-cycle deferral.
  A stale open JSON sidecar can no longer keep the idle boundary waiting after a
  terminal lazily closeout projection.
- The file-IPC live-prompt-drift path no longer treats response-bearing
  editor receipt content as logical snapshot authority when it also carries
  user-owned drift that appeared after preflight. A lazily applied receipt can
  prove a new response heading is visible and suppress editor redelivery, but
  the saved snapshot comes from the agent-owned `content_ours` image unless a
  semantic/forward merge constructs a proven converged image. Live queue text
  may be preserved when it is part of `content_ours`; post-exchange scratch
  typing remains editor-visible but uncommitted when it is not.
- Queue cooldown is complete: queue context-clear state now records deferred,
  started, settled, and manual cooldown facts in the backbone/controller
  projection. `.agent-doc/queue-cooldowns` is no longer a hot-path authority.
- ACK compatibility transport is removed from the active contract. Maintained
  JetBrains and VS Code plugins publish lazily receipt-backed visible-write
  content through `agent_doc_editor_content_applied_for_editor_v1` with
  `lazily_transport_receipts_v1`; legacy `agent_doc_write_ack_content*` ABIs
  fail closed with an update/reinstall error instead of seeding a compatibility
  projection.
- Socket IPC immediate responses are now typed receipts, not ACKs. The
  maintained listener returns `{"type":"receipt","status":"applied"}` /
  `{"type":"receipt","status":"rejected"}`, optional
  `reason:"already_applied"` remains a receipt reason, and early liveness uses
  `{"type":"receipt","status":"accepted"}` with the `early_receipt` opt-in.
  Legacy `{"type":"ack",...}` listener output is classified as unsupported and
  fails closed with an update/reinstall error instead of acting as compatibility
  proof.
- Socket and file IPC closeout now require lazily visible-write receipts for
  editor-applied content. Patch-file deletion remains only a file-IPC
  consumption signal; it is not delivery proof, and ack-content sidecars are no
  longer fallback snapshot authority.
- File-IPC rejection now uses durable lazily `EditorPatchRejected` receipts.
  Maintained JetBrains and VS Code plugins record the rejection through
  `agent_doc_editor_patch_rejected`, the FFI writes the receipt into the
  project backbone, and the file-IPC poller fails closed from that projection.
  `.nack` files are no longer written or read as active negative-delivery
  authority.
- ACK primitive decision: lazily should not add a ZeroMQ-style special ACK
  channel for agent-doc. The useful primitive is a generic causal receipt fact
  and projection (`observed` / `accepted` / `applied` / `rejected`) that agent-doc
  can use to model domain-specific `EditorPatchApplied`,
  `EditorPatchRejected`, and `VisibleWriteCommitCandidateObserved` facts.
  `EditorPatchApplied` / `EditorPatchRejected` remain agent-doc semantics; lazily
  supplies the receipt/projection substrate.
- Lazily release status as of 2026-07-05: `lazily-spec v0.22.0`,
  `lazily-formal v0.23.0`, `lazily-rs v0.21.5`, `lazily-kt v0.12.1`, and
  `lazily-js v0.5.2` are tagged and pushed. `lazily-rs v0.21.5` is published to
  crates.io with direct `ctx.get(factory)` / `ctx.set(factory, value)` typed
  factory APIs, and `lazily-kt v0.12.1` is published to GitHub Packages. The
  `@lazily-hub/lazily-js v0.5.2` npm publish is blocked on npm OTP/browser auth
  (`EOTP`); the package is otherwise built, tested, tagged, and ready to publish
  after npm 2FA is completed.
- Agent-doc backfill status as of 2026-07-05: socket/file IPC and typed
  `save_document` now use lazily visible-write receipts/projections as proof.
  `.agent-doc/ack-content` is only a legacy-negative diagnostic/rejection path,
  the merge-state `ipc_ack_proven` serde alias is removed, and the workflow
  doctor treats legacy ack-content directory presence as ignored diagnostic
  state rather than editor proof.
- Remaining `ack_mismatch`, `no_ack`, and route/cycle "ack" strings are legacy
  diagnostic or non-editor startup-handshake vocabulary. They are not active
  editor-plugin compatibility transports; rename them only with a separate
  migration of persisted reason tokens and static tests.
- Semantic-merge acknowledgement carry-forward is now a closeout-backbone
  projection rather than a cycle-state sidecar payload. `record_semantic_merge_acks`
  appends `SemanticMergeAckRecorded` facts, `start_preflight` carries un-surfaced
  projection facts forward with `SemanticMergeAckCarriedForward`, and preflight
  surfaces notices through the projection-aware reader. The JSON
  `pending_semantic_merge_acks` field remains a compatibility projection and is
  imported only when it matches the projected cycle. The read/carry fallback
  chain is centralized through named projection and `CycleState` helpers so
  sidecar compatibility does not duplicate the ack lifecycle transform.
- Repair pending-response replay now reads the lazily pending-response
  projection first. The `.agent-doc/pending/*` file is only imported when no
  durable pending/clear fact exists for the document; a `WriteApplied` or
  `PendingResponseCleared` projection suppresses stale backup replay even if the
  file is still present, and repair retires the stale backup file without
  replaying it.
- Repair cycle-phase decisions now use the projection-aware cycle view for
  visible-response adoption, already-applied response cleanup, explicit
  preflight cancellation, stale-preflight repair, and missing commit-boundary
  recovery. Pending-response clear events also source cycle/capture identity
  from closeout projection facts when the compatibility JSON sidecar is stale or
  absent.
- Write/finalize committed-cycle guards now use
  `agent_doc_cycle_state_io::load_with_closeout_projection` for terminal phase
  decisions. A stale open cycle JSON sidecar can no longer hide a committed
  closeout projection from explicit-baseline replay protection, IPC timeout
  cleanup, terminal closeout guards, pipeline frontmatter mirrors, or IPC
  payload cycle identity. Terminal closeout proof payloads now append a typed
  `TerminalCloseoutProofRecorded` fact with file/snapshot/HEAD hashes and
  agreement metadata, and structured terminal-proof assertions use
  `load_latest_terminal_closeout_proof` instead of reparsing the proof ledger
  row. The proof ledger row remains a compatibility/audit projection. Closeout
  recovery evidence observation now appends a typed
  `CloseoutRecoveryEvidenceRecorded` fact with visible/snapshot hashes, active
  cycle/capture state, response-body proof, queue-only drift proof,
  editor-IPC freshness, and binary freshness metadata. Recovery decisions now
  read that projection before recomputing sidecar evidence when the projected
  visible markdown hash still matches the current file. Missing-response
  recovery classification also uses the current observed recovery-evidence
  projection before falling back to `stuck_captured_cycle`, so a missing capture
  sidecar no longer hides projected `empty` / `missing` / `superseded`
  response-body evidence. Snapshot-vs-HEAD and snapshot-vs-visible drift
  classifications are now recorded in that same projection and recovery
  classification consumes those projected drift classes before reading the
  snapshot sidecar. Remaining write/finalize work is to extend that load-bearing
  projection use to the remaining detailed classifier/payload readers before
  reconstructing evidence from compatibility records.
- Closeout recovery now resolves and mutates current/visible document text through
  `CloseoutEffects::resolve_current_document`, implemented by orchestration with
  `agent_doc_document_realtime_io::try_resolve_current_doc_from_file`, and
  `CloseoutEffects::write_current_document`, implemented with
  `atomic_write_through_authority`. Recovery evidence gathering, observed-evidence
  freshness checks, snapshot-vs-visible classification, metadata-drift recovery,
  terminal proof file hashes, and restore-from-HEAD visible writes no longer touch
  the session document directly on disk; disk is used only inside the realtime
  resolver/writer after editor authority is accounted for.
- Direct document-I/O audit started for `std::fs::read_to_string(file)` /
  `std::fs::write(file, ...)` call sites. The orchestration preflight path now
  resolves current text through `agent_doc_document_realtime_io` for initial
  parsing, deprecated frontmatter cleanup, prompt-tail relocation, cycle start
  payloads, model/frontmatter/warning scans, open-backlog checks, and owner-pane
  self-invocation detection; the preflight helper repairs also use the same
  current-document resolver. Session-check now has a crate-local
  `resolve_current_document` helper and the direct-patchback / JB cache-conflict /
  late-IPC overapplication / unstarted prompt-bearing detectors and committed
  HEAD-response visibility checks use it. Remaining direct document-I/O clusters
  are still load-bearing in commit, write, write-converge, repair, preflight-io
  maintenance, sync helpers outside sync-status/frontmatter writeback, resync
  finish-turn progress hashing, and pane-repair harness resolution, and
  command-only utilities outside focused status writeback, claim, focused start
  preparation/runtime helpers, queue-continuation production callers, and
  session-check committed-response visibility, closeout pending-maintenance, and
  Codex Stop-hook queue recovery, document hook frontmatter reads, and prompt
  command IO, boundary command IO, response-TOC command IO, and response-replay
  dedupe command IO, finalize lint-gate IO, read command IO, outline command
  IO, and read-only root command adapters for `auto-dag`, cleanup fallback,
  layout, parallel, and plan rendering, plus root mutation command adapters for
  convert, exchange, migrate, mode, and patch, notify/history mutation
  adapters, queue-dispatch context IO, reset command IO, and session-actor
  committed-cycle dirty checks, sync status/frontmatter warning writeback,
  resync finish-turn progress hashing, and pane-repair harness resolution.
  Route/orchestrate handoff, ordered-task dispatch, backlog command IO,
  queue-consume helper edges, claim command IO, boundary command IO,
  stream/compact adapters, queue-continuation production callers, Codex Stop-hook
  queue recovery, document hook frontmatter reads, prompt command IO,
  response-TOC command IO, response-replay dedupe command IO, finalize lint-gate
  IO, read command IO, outline command IO, read-only root command adapters for
  `auto-dag`, cleanup fallback, layout, parallel, and plan rendering,
  root mutation command adapters for convert, exchange, migrate, mode, and
  patch, notify/history mutation adapters, queue-dispatch context IO,
  reset command IO, session-actor committed-cycle dirty checks,
  sync status/frontmatter warning writeback, resync finish-turn progress hashing,
  pane-repair harness resolution, session-check guard-command production reads,
  and the write-runtime
  `run_entry` / write-IPC prewrite, payload, and retry helpers have started
  using the current-document resolver/effect boundary. Each remaining direct
  access must be classified as either realtime-current document access, explicit
  detached-disk/audit access, or sidecar/metadata IO; the current remaining
  root-crate raw-file matches are classified as artifact JSON/log scanning in
  `ops_report`, bootstrap document creation in `init`, and `#[cfg(test)]`
  orchestration fakes.
- Closeout recovery diagnostics now use the projection-aware cycle view for
  stuck-captured-cycle detection, compacted committed-capture reconciliation,
  recovery-state classification, evidence summaries, and open-cycle recovery
  command payloads. A stale cycle sidecar cannot hide a committed lazily
  projection from those preflight/session-check recovery decisions, and a
  projection-only open cycle still renders cycle/phase recovery guidance instead
  of falling through to `clean`. Projection-only preflight fails closed as
  `open_cycle` because the JSON-only pending-mutation detail is no longer
  authoritative enough to prove `open_empty_preflight`.
- Session-check closeout signal guards now use the projection-aware cycle view
  for blocked follow-up, gated-phase split, queue-audit partial completion,
  pending-response/done, response-shape, queue-head/provenance, partial-staging,
  write-time pending maintenance, recoverable cache-conflict detection, and the
  smaller continuation/manual-patchback/repaired-cycle checks in
  `session-check` command flow. They still use compatibility cycle fields for
  detailed pending/backlog payloads until those facts move into the backbone,
  but lazily phase/capture state wins over stale open JSON for guard evaluation.
- Captured response guard payloads now have a typed closeout projection. The
  `ResponseCaptured` closeout fact can carry the redacted response body,
  `agent-doc-capture-io::capture_response` appends that payload into the state
  backbone, and session-check guards for blocked follow-up, gated-phase split,
  queue audit partial completion, pending capture/done, queue-head provenance,
  partial closeout state, and write-time pending checks read the projected body
  before falling back to capture JSON. A stale or missing capture sidecar can no
  longer suppress those guards when matching lazily cycle/capture/body facts are
  present.
- Closeout recovery evidence gathering and stuck-captured-cycle detection now
  read captured response text from the typed closeout projection before capture
  JSON. Fresh recovery evidence and stuck-cycle warnings can still prove a
  matching response body after the capture sidecar is missing. Captured-response
  projection facts now also carry the response-time visible/snapshot baseline
  hashes, so fresh queue-only drift proof no longer requires compatibility
  capture JSON when matching lazily cycle/capture/body/hash facts are present.
- Repair replay validation now uses the realtime-resolved current document that
  repair already loaded instead of asking capture IO to reread `file` from disk.
  `agent-doc-capture-io` exposes `validate_replay_with_current_content` and
  `replay_baseline_drifted_with_current_content`, and repair uses those helpers
  for already-applied dedupe validation, stale-capture retirement evidence, and
  final replay-baseline validation. Repair commit-boundary recovery and
  committed-historical repair also mark active capture state with
  `mark_committed_with_current_content` so capture hash refreshes use the same
  resolved current document. The legacy disk-reading wrappers remain for
  unmigrated callers; the repair hot path no longer treats disk as the active
  document authority for capture replay/commit checks.
- Post-commit capture closeout now threads the same resolved current document
  content used by `agent-doc-git-io` post-commit cleanup into capture state:
  `PostCommitCleanupEffects::mark_capture_committed` requires caller-supplied
  current content, and `agent-doc-commit-io` calls
  `mark_committed_with_current_content` rather than letting capture IO reread
  `file` from disk. The sync test runtime's pending/commit helpers also pass
  current content into capture IO, keeping the compatibility wrappers isolated
  to older or explicitly unmigrated callers.
- Commit materialization guards now use the same typed captured-response
  projection before active capture JSON. The already-current / staged-snapshot
  guard and active response-target classifier can still fail closed on a missing
  captured response when the capture sidecar has gone stale or missing, as long
  as the lazily cycle/capture/body projection matches.
- Queue-consume commit seam proof now reads captured response text from the
  typed closeout projection before capture JSON. Answered free-text head
  detection and prompt-drift response targeting can consume a matching
  lazily cycle/capture/body projection even after the active capture sidecar is
  stale or missing; capture JSON is retained only as a compatibility fallback.
- Post-response hook metadata now reads active capture payloads from the typed
  closeout projection before active capture JSON. `post_write` / `post_commit`
  hook metadata and tsift memory closeout can still receive the matching
  capture id, response hash, and response body when capture JSON is missing;
  capture JSON remains the compatibility fallback for older projection gaps.
- Document hook frontmatter reads now resolve the active session document
  through `agent_doc_document_realtime_io::try_resolve_current_document_content`
  before parsing configured `session_start`, `post_write`, and `post_commit`
  hooks. Hook dispatch therefore sees the editor-current document model when an
  editor owns the buffer instead of probing stale disk for hook configuration,
  and static coverage rejects raw `std::fs::read_to_string(file)` in
  `agent-doc-hooks-io`.
- Prompt command IO now resolves the active session document through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` before
  extracting the frontmatter session id for prompt polling and prompt-answer
  navigation. Prompt parsing and tmux navigation stay in the focused tmux
  adapters, but `agent-doc-prompt-io` no longer probes stale disk to decide the
  target session. Static coverage rejects raw `std::fs::read_to_string(file)` in
  `agent-doc-prompt-io`.
- Boundary command IO now resolves and mutates the active session document
  through `agent_doc_document_realtime_io` before inserting a boundary marker.
  The command still owns the boundary-specific editor refresh signal, but it no
  longer reads stale disk or uses a temp-file rename as the document write
  authority. Static coverage rejects the old raw read/temp-write/rename pattern
  in `agent-doc-boundary-io`.
- Response-TOC command IO now resolves live response sections through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` for both
  `response-toc` live entries and `response-fetch live:<n>` windows. Archive
  lookups stay on the sqlite archive index, but live/current response inspection
  no longer treats disk as the hot-path document authority. Static coverage
  rejects raw active-document reads in `agent-doc-response-toc-io`.
- Response-replay dedupe command IO now resolves the active session document
  through `agent_doc_document_realtime_io::try_resolve_current_document_content`
  before applying focused duplicate-response policy. The deduped write and
  snapshot update remain injected command effects, but the command no longer
  uses stale disk as the read authority for deciding whether a duplicate response
  exists. Static coverage rejects raw active-document reads in
  `agent-doc-response-replay-io`.
- Finalize lint-gate IO now uses an explicit read-authority split before
  applying frontmatter/project lint-mode policy and the in-process tagpath
  dialect check: normal finalize resolves through
  `agent_doc_document_realtime_io::try_resolve_current_document_content`, while
  `--force-disk` finalize calls the named
  `resolve_disk_current_document_content` path. Tagpath and lint-mode ownership
  remain in their focused crates, and static coverage rejects raw active-document
  reads in `agent-doc-lint-io`.
- Read command IO now resolves the active session document through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` before
  printing the full document or parsing a named component body. Static coverage
  rejects raw active-document reads in `src/read.rs`.
- Outline command IO now resolves the active session document through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` before
  rendering markdown outline output. Static coverage rejects raw active-document
  reads in `src/outline_cmd.rs`.
- Read-only root command adapters for `auto-dag`, cleanup fallback
  summarization, layout, parallel, and plan rendering now resolve active session
  documents through
  `agent_doc_document_realtime_io::try_resolve_current_document_content`.
  Schedule/log sidecar reads remain ordinary file IO, while static coverage
  rejects raw active-document reads in `src/auto_dag.rs`, `src/cleanup_cmd.rs`,
  `src/layout.rs`, `src/parallel.rs`, and `src/plan.rs`.
- Root mutation command adapters for convert, exchange, migrate, mode, and patch
  now resolve active session documents through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` and
  write document mutations through
  `agent_doc_document_realtime_io::atomic_write_through_authority`. Snapshot and
  schedule sidecar writes remain ordinary file IO. Static coverage rejects raw
  active-document reads/writes in `src/convert.rs`, `src/exchange.rs`,
  `src/migrate.rs`, `src/mode.rs`, and `src/patch.rs`.
- Queue-dispatch context IO now resolves session/frontmatter routing state
  through `agent_doc_document_realtime_io::try_resolve_current_document_content`
  before selecting supervisor/tmux dispatch paths. Notify and history restore
  command mutations also resolve active session documents through the realtime
  resolver and write document mutations through
  `agent_doc_document_realtime_io::atomic_write_through_authority`; git object
  reads and snapshot sidecar writes remain ordinary file IO. Static coverage
  rejects raw active-document reads/writes in `src/queue_dispatch.rs`,
  `src/notify.rs`, and `src/history.rs`.
- Reset command IO now routes normal and sidecar-rebuild current-document reads
  through `agent_doc_document_realtime_io::try_resolve_current_document_content`
  while keeping the explicit `--force-disk` escape hatch on the named
  `resolve_disk_current_document_content` path. Session-actor committed-cycle
  dirty checks now read the projection-aware committed cycle phase/hash and
  compare the stored committed hash against realtime current-document authority
  instead of raw disk. Static coverage rejects raw active-document reads in
  `src/reset.rs` and `src/session_actor_cmd.rs`.
- Sync status/frontmatter warning writeback now resolves and mutates the active
  session document through the installed `SyncRuntimeEffects` document authority
  callbacks. The CLI binds those callbacks to
  `agent_doc_document_realtime_io::try_resolve_current_document_content` and
  `agent_doc_document_realtime_io::atomic_write_through_authority`, while tests
  keep a disk-backed effect implementation. Static coverage rejects the old raw
  production status read/write path in `agent-doc-sync-io/src/lib.rs` and
  verifies the CLI realtime binding.
- Resync finish-turn progress hashing and sync pane-repair harness resolution now
  read active session documents through the same `SyncRuntimeEffects` document
  authority callbacks instead of raw disk. Static coverage rejects raw production
  active-document reads in `agent-doc-sync-io/src/resync.rs` and
  `agent-doc-sync-io/src/sync/pane_repair.rs` while preserving disk-backed test
  effects.
- Write-runtime queue closeout and write-converge removal detection now read the
  captured response body from the typed closeout projection before active
  capture JSON. Queue-consumption permission, answered free-text strikes, and
  active-response-removed convergence checks keep working when the matching
  lazily cycle/capture/body projection exists but the capture sidecar is missing.
- Missing-pane closeout recovery diagnostics now use the projection-aware cycle
  view and check the typed captured-response projection before active capture
  JSON when reporting durable capture presence. A stale or missing capture
  sidecar no longer makes sync recovery logs claim `durable_capture=false` when
  the matching lazily cycle/capture/body projection exists. Inspect-only
  missing-pane repair phase reporting, dead-pane phase labels, and open-cycle
  pane protection helpers also use the projection-aware cycle view, so stale
  open JSON cannot report or protect a pending phase after a terminal closeout
  projection exists.
- Orchestrate route dispatch now closes preflight handoff cycles through the
  projection-aware cycle view and snapshots the current document resolver image
  rather than raw disk. Ordered task dispatch also parses frontmatter from the
  current-document resolver after task injection, and fresh-agent launch /
  streaming launch frontmatter resolution, batch-change closeout, and prompt
  injection use the same resolver. Route task discovery for `--from-exchange`,
  `--from-queue`, prompt-preset canonicalization, prompt preset block loading,
  and exchange-source mutation checks also resolve current document content
  through the realtime boundary instead of treating disk as the hot-path content
  authority.
- Route queue dispatch and route document preparation now resolve active session
  documents through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` before
  queue enqueue/activation, inactive-head checks, session-frontmatter
  preparation, and duplicate prompt cleanup. Existing route write effects still
  own convergence/write authority, while static coverage rejects raw
  `std::fs::read_to_string(file)` active-document probes in
  `agent-doc-route-io/src/queue_dispatch.rs` and
  `agent-doc-route-io/src/document_prep.rs`.
- Remaining route startup/dispatch closeout readers now use the same realtime
  document authority for startup harness resolution, managed capability proof
  frontmatter checks, authoritative actor expected-harness matching, and
  closeout-block active queue-head classification. Static coverage rejects raw
  active-document reads in `agent-doc-route-io/src/startup_harness.rs`,
  `agent-doc-route-io/src/authoritative_actor.rs`, and
  `agent-doc-route-io/src/closeout_drain.rs`; the remaining route raw write is
  a `#[cfg(test)]` effect fixture.
- Cycle-state pipeline frontmatter mirrors no longer read the active document
  inside `agent-doc-cycle-state-io`. The `PipelineFrontmatterEffects` port now
  requires a current-document read, and the runtime implementation resolves it
  through
  `agent_doc_document_realtime_io::try_resolve_current_document_content`.
  Phase-event session logging consumes caller-supplied current content instead
  of re-reading `file`; progress-only transitions without current content skip
  the best-effort log rather than treating disk as authority. Static coverage
  pins the port/realtime split and rejects raw production active-document reads
  in `agent-doc-cycle-state-io/src/pipeline_frontmatter.rs` and the
  cycle-state phase logger.
- Status writeback now projects from `StatusWriteEffects::current_document_content`
  on the normal `write --status` path, implemented by
  `agent_doc_document_realtime_io::try_resolve_current_document_content`, before
  handing the mutation to write convergence. The `--force-disk` branch resolves
  through the explicit disk-authority helper and still records provenance and
  audit logging. A regression covers an editor-current status body being
  preserved while stale disk status text is ignored, and the static ownership
  guard now requires the realtime/disk read-authority split in
  `agent-doc-status-io`.
- Direct-run startup repair now classifies its malformed-frontmatter fix as an
  explicit detached-disk read and authority-routed write: the helper reads
  through `agent_doc_document_realtime_io::resolve_disk_current_document_content`
  and persists the repaired frontmatter with
  `agent_doc_document_realtime_io::atomic_write_through_authority` instead of
  raw `std::fs` document I/O. Focused regressions cover tab-indentation repair
  and missing-file no-op behavior, and the direct-run static guard now rejects
  the old raw `read_to_string(file)` / repair `write(file, ...)` pattern.
- Start runtime preparation now resolves the startup document through
  `agent_doc_document_realtime_io::try_resolve_current_document_content`, writes
  generated session frontmatter with
  `agent_doc_document_realtime_io::atomic_write_through_authority`, and routes
  crash-replayed queue-item merges through the same write authority. The
  start-IO static guard rejects the old raw startup `read_to_string(file)` and
  generated-session / queue-replay `write(file, ...)` patterns.
- Start-runtime restart and idle-watch helpers now read through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` for
  restart frontmatter, HEAD convergence, active queue head detection, and
  forced context-reset reason checks. A regression proves an unsaved
  editor-current buffer blocks HEAD convergence even when disk still matches
  `HEAD`, and the start-runtime static guard rejects the old raw
  `read_to_string(file)` shape in `run.rs` and `idle_watch.rs`.
- Backlog command IO now reads the active session document through injected
  `BacklogCommandEffects` instead of direct `std::fs::read_to_string(file)`.
  The focused `agent-doc-element-backlog-runtime-io` provider maps normal
  command reads to
  `agent_doc_document_realtime_io::try_resolve_current_document_content` and
  maps `--force-disk` reads to
  `agent_doc_document_realtime_io::resolve_disk_current_document_content`; the
  remaining direct `std::fs::write(file, ...)` in `backlog_cmd.rs` is the
  explicit detached-disk `--force-disk` write branch. CLI backlog/review
  commands now reuse that focused runtime provider instead of a local duplicate,
  and static ownership coverage rejects raw command document reads and
  CLI-local backlog runtime effects.
- Queue-consume helper edges no longer expose file-reading adapters for
  diff-based consume decisions, skip diagnostics, or active-head response
  targeting. Direct-run, write-runtime, and Codex Stop-hook callers now resolve
  current document content through their existing realtime boundary and call the
  pure queue policy on that content. `QueueConsumeWriteEffects::force_disk_document_content`
  is implemented explicitly by the CLI and write-runtime force-disk paths via
  `agent_doc_document_realtime_io::resolve_disk_current_document_content`, so a
  force-disk queue consume is a named detached-disk read rather than a raw
  `std::fs::read_to_string(file)` fallback. Static ownership coverage rejects
  the removed queue file-reading adapters and requires runtime callers to resolve
  current content before queue diagnostics.
- Codex Stop-hook queue recovery now resolves active document content through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` before
  active-session queue prompt detection, repeated queue response recovery, and
  auto-close queue-completion proof. The remaining Stop-hook queue policy still
  calls pure queue helpers, but it no longer probes the active session document
  on disk to decide queue head identity or completion ids. Static coverage
  rejects reintroducing raw `std::fs::read_to_string(file)` in
  `agent-doc-codex-stop-io`.
- Stream command IO now reads the active session document through
  `StreamRuntimeEffects::current_document_content` instead of direct
  `std::fs::read_to_string(file)`, and the CLI runtime maps those reads to
  `agent_doc_document_realtime_io::try_resolve_current_document_content`.
  Session-id frontmatter creation, resume-id updates, stream content-ours
  fallback, and direct flush fallback all route through the same runtime
  authority boundary; the generated session frontmatter write continues through
  `StreamRuntimeEffects::atomic_write`, implemented by the CLI with
  `agent_doc_document_realtime_io::atomic_write_through_authority`. Static
  coverage rejects the removed raw stream document read/write patterns and
  requires the CLI stream effects provider to bind current-document reads to
  realtime IO.
- Stream final-response capture now also uses the runtime current-document
  authority: `agent-doc-capture-io` exposes
  `capture_response_with_current_content`, `agent-doc-repair-io` exposes
  `save_pending_with_current_content`, and CLI stream/sync save-pending paths
  pass realtime-resolved current content before writing capture/pending
  projections. The legacy `capture_response(file, response)` wrapper remains for
  unmigrated callers, but stream capture no longer forces capture IO to reread
  the active document from disk.
- Stream partial-response checkpoints now have the same current-content escape
  hatch: `PartialCheckpointWriter::maybe_checkpoint_with_current_content`
  persists checkpoint metadata from caller-supplied current document content,
  and both `agent-doc-stream-io` plus orchestrated exchange streaming resolve
  current content through realtime IO before calling capture IO. The legacy
  `maybe_checkpoint` wrapper remains for old tests/callers; stream hot paths no
  longer ask capture IO to read disk just to hash/parse checkpoint baselines.
- Compact command IO now reads the active session document through
  `CompactRuntimeEffects::current_document_content` on the normal path and
  `CompactRuntimeEffects::force_disk_document_content` only for named
  detached-disk probes: `--force-disk` initial parse, editor-buffer flush
  polling, and post-write working-tree verification before commit. The CLI
  runtime maps normal reads to
  `agent_doc_document_realtime_io::try_resolve_current_document_content`, maps
  force-disk probes to
  `agent_doc_document_realtime_io::resolve_disk_current_document_content`, and
  keeps compact writes on
  `agent_doc_document_realtime_io::atomic_write_through_authority`. Static
  coverage rejects the removed raw compact document reads and requires the CLI
  compact effects provider to bind current vs. disk authority explicitly.
- Claim command IO now reads and mutates the active session document through
  `ClaimRuntimeEffects::current_document_content` and
  `ClaimRuntimeEffects::atomic_write` instead of direct
  `std::fs::read_to_string(file)` / `std::fs::write(file, ...)` calls. Empty-file
  auto-scaffolding, session UUID materialization, default template/CRDT
  frontmatter insertion, and default component scaffolding all route through
  the injected authority boundary; the CLI provider maps reads to
  `agent_doc_document_realtime_io::try_resolve_current_document_content` and
  writes to `agent_doc_document_realtime_io::atomic_write_through_authority`.
  Static coverage rejects the removed raw claim document reads/writes while
  leaving claims-log and registry sidecar IO as ordinary sidecar/metadata IO.
- Repair-runtime missing-committed-response and dedupe-only recovery callbacks
  now read the active session document through
  `agent_doc_document_realtime_io::try_resolve_current_document_content` instead
  of direct `std::fs::read_to_string(file)`. Those runtime callbacks already
  guard and write through `agent_doc_document_realtime_io` before snapshot/commit
  closeout, and static coverage rejects reintroducing raw active-document reads
  in `agent-doc-repair-runtime-io`.
- Repair core's `--force-disk` override path now reads through
  `agent_doc_document_realtime_io::resolve_disk_current_document_content` instead
  of direct `std::fs::read_to_string(file)`, while the normal repair path keeps
  using `try_resolve_current_document_content`. This makes the repair read
  authority split explicit: realtime-current for normal repair and named
  detached-disk authority only for force-disk repair overrides. Static coverage
  rejects raw active-document reads in `agent-doc-repair-io`.
- Write-runtime `run_entry` now routes its active document reads through named
  current-vs-disk authority helpers. Normal append/template/stream prewrite and
  merge checks, IPC consumed-content checks, and repair apply helpers use
  `agent_doc_document_realtime_io::try_resolve_current_document_content`; explicit
  `--force-disk` append/template/stream prewrite and merge checks, force-disk
  repair apply checks, and the file-IPC payload baseline reference use
  `agent_doc_document_realtime_io::resolve_disk_current_document_content`. Static
  coverage rejects raw active-document reads and direct bypasses of the helper
  split in `agent-doc-write-runtime-io/src/run_entry.rs`, and a force-disk
  finalize regression covers an editor-attached/model-missing document staying
  on the named disk-authority path instead of failing through realtime-current
  resolution.
- Write-IPC payload and retry transport now routes active session-document reads
  through a focused `agent-doc-write-ipc-io` authority helper. Patch synthesis,
  reposition payload baseline/working-document reads, socket/file IPC baseline
  capture, mid-turn cycle-state refresh, file-IPC dedupe, and post-delivery
  before-content proof first try
  `agent_doc_document_realtime_io::try_resolve_current_document_content` and then
  fall back through named `resolve_disk_current_document_content` sources where
  IPC retry material is allowed to use the visible file as explicit
  detached-disk evidence. Static coverage rejects raw active-document reads in
  `agent-doc-write-ipc-io/src/lib.rs` and `src/transport.rs`, while preserving
  ordinary patch-file sidecar reads. The IPC timeout retry regression covers an
  editor-attached/model-missing case retaining its queued patch for retry instead
  of losing the response before file IPC can pick it up.
- Queue-continuation production callers now resolve active document content at
  their runtime/command boundary and pass it into
  `agent_doc_queue_io::queue_continuation::detect_for_content`, which keeps
  queue continuation policy, snapshot loading, and recycle-yield handling in
  queue IO without letting those callers fall back to a raw file read.
  Direct-run owned-pane deferral, session-check continuation reporting, route
  busy-autoloop deferral, MCP session-check reporting, and session-actor
  busy-clear preemption all use named realtime document-authority sources before
  evaluating continuation. Static coverage requires those production callers to
  use the content-based detector and leaves the file-backed detector as a
  compatibility wrapper for test and legacy callers.
- Session-check committed-response visibility now compares `HEAD` response
  headings against content from the crate-local current-document resolver instead
  of a preflight raw disk probe. The live-buffer tolerance for a latest committed
  response that is visible in the editor, and the missing-response interruption
  when it is not visible, now share the same realtime-current authority path.
  Static coverage rejects reintroducing direct `std::fs::read_to_string(file)`
  in `agent-doc-session-check-io/src/command.rs`.
- Template IO no longer reads or writes active session documents directly for
  template-info rendering, pending/backlog patch normalization, response-capture
  canonicalization, or partial materialization retry diagnostics. Those readers
  resolve through
  `agent_doc_document_realtime_io::try_resolve_current_document_content`; the
  pending/backlog normalization write keeps the visible-write idle guard and then
  writes via `agent_doc_document_realtime_io::atomic_write_through_authority`.
  Static coverage pins the realtime read/write calls and rejects direct active
  document `read_to_string(file)` / `write(file, ...)` patterns in the template
  IO production files.
- Closeout pending-maintenance gating now records whether the preflight-current
  document already required tracked-work maintenance as an optional typed
  `PreflightStarted` / closeout-projection fact. `session-check` consumes that
  fact before reading document content: `Some(true)` requires maintenance,
  `Some(false)` authorizes status-only closeout skips, and missing legacy facts
  fail through the current-document resolver without probing disk when editor
  authority is unavailable. The previous non-authoritative disk negative probe
  in `write_pending_checks.rs` is removed, and static coverage rejects raw
  production `read_to_string(file)` probes in that module while preserving test
  fakes.
- Direct-run no-change, owner-pane queue-edit defer, and dispatch-timeout
  diagnostics now read cycle phase/capture state through
  `load_with_closeout_projection` before compatibility cycle JSON. A stale open
  cycle sidecar can no longer be the sole authority for these direct-run
  decisions after the lazily closeout projection has moved the cycle terminal.
  Static ownership coverage pins these readers to the projection-aware loader
  while leaving transition writers free to maintain the JSON projection.
- Route closeout drain now also uses `load_with_closeout_projection` for the
  initial open-cycle decision and the concurrent-finalize retry re-read. Route
  dispatch therefore observes lazily terminal/advanced closeout facts before a
  stale open JSON sidecar when deciding whether to drain, retry, or unblock
  dispatch. Route cycle-ack polling, pane-resolution baselines, and fresh-start
  startup baselines also use the projection-aware cycle view, so stale open JSON
  cannot hide an acknowledged projected closeout or feed stale baseline state
  into route dispatch policy.
- Session-check command closeout handling now uses the shared
  `load_with_closeout_projection` reader instead of manually loading the cycle
  JSON and applying a private projection overlay. Projection-only open-cycle
  diagnostics still use `load_closeout_projection`, but sidecar-backed phase and
  capture decisions now flow through the same projection-aware loader as the
  focused guard adapters.
- State-observer advisory closeout snapshots now also use the projection-aware
  cycle view, so `adstatechart_snapshot` observability follows committed or
  abandoned closeout facts even when the compatibility cycle JSON is stale-open.
- Ops-log turn attribution now reads the latest closeout projection before the
  compatibility cycle sidecar. Diagnostic lines therefore carry the lazily
  projected turn id even when the JSON sidecar is stale on an older cycle.
- The editor-facing FFI turn projection now reads the projection-aware cycle
  view before mapping cycle phase into the CPC turn projection. A stale open
  cycle sidecar can no longer make plugins render an in-flight turn after a
  terminal closeout projection exists.
- Serve active-response polling and browser save routing now use the
  projection-aware cycle view before compatibility cycle JSON. A stale open
  sidecar can no longer make the web UI keep streaming a partial response or
  send browser edits through `write --commit` after a terminal closeout
  projection exists.
- Response-capture IO now uses the projection-aware cycle view for partial
  checkpoint writer cycle ids, checkpoint acceptance, capture ids, and active
  capture lookup. A stale open cycle sidecar can no longer keep partial
  checkpoints flowing after a committed closeout projection exists.
- Write IPC payload and diagnostic cycle-id tags now read the latest closeout
  projection before the compatibility cycle sidecar. Late socket/file IPC
  payloads therefore fence against the projected current turn rather than an
  older stale cycle JSON row.
- Preflight route-queue snapshot recovery now uses the projection-aware cycle
  view before deciding whether an existing cycle is still open. Stale open JSON
  can no longer suppress snapshot-boundary auto-recovery after a terminal
  closeout projection exists.
- Workflow doctor cycle-state diagnostics now use the projection-aware cycle
  view, so doctor output reports lazily terminal closeout facts instead of stale
  open compatibility JSON.
- Commit IO post-commit cleanup effects now use the projection-aware cycle view
  for terminal-cycle checks before falling back to compatibility sidecar state.
- Project-controller session-actor closeout persistence now uses the
  projection-aware cycle view before writing closeout facts into SQLite. Stale
  open cycle JSON can no longer make the controller persist an open phase after
  a committed closeout projection exists.
- The shared Lazily `CycleContext` cycle-state slot now reads through the
  projection-aware loader, so callers of `rc.cycle_state()` observe terminal
  closeout facts even when the compatibility sidecar is stale-open.
- Watch-daemon in-flight suppression now uses the projection-aware cycle view
  before applying freshness policy. Stale open cycle JSON can no longer suppress
  reactive watch retries after a terminal closeout projection exists.
- Route-owned supervisor completion polling now loads cycle state through a
  projection-aware helper before deciding whether a route-owned cycle has
  committed since startup. Stale open sidecars can no longer keep the owned pane
  completion loop waiting after terminal closeout projection exists.
- Session-actor debug cycle-phase reporting now reads the projection-aware
  cycle view, so the debug dump reports terminal closeout facts instead of stale
  open compatibility JSON.
- Write-converge live-prompt drift recovery and visible-write payload cycle tags
  now read projected closeout state before compatibility cycle JSON. Late
  convergence retries therefore attach to the latest projected turn instead of
  an older stale-open sidecar.
- Preflight command open-cycle checks and the session-check
  committed-without-response-body guard now read through the projection-aware
  cycle loader. These production hot paths no longer re-read raw cycle JSON for
  closeout phase or guard evidence after a projected closeout exists.
- Current observed closeout-recovery evidence now validates the typed projection
  against live visible-document authority only. A stale compatibility snapshot
  baseline can no longer veto a current typed observation; the snapshot hash
  retained in that observation remains advisory recovery evidence.

The queue/supervisor bullets are Phase 3 sidecar-retirement slices for narrow
gates. The Phase 1 hook closes the first raw preflight failure shape, but it is
not the full desired model yet. Lifecycle writers now feed the backbone, and
session-check, repair, preflight/write-entry, and supervisor gates have
closeout-projection reader cutovers for phase and capture authority. Remaining
work is to make the newly typed terminal proof/recovery evidence projections
load-bearing for all recovery classifiers/detail payloads and to move the
remaining detailed guard payload facts to typed backbone state before falling
back to snapshot, capture, and cycle-state sidecars as compatibility
projections.

## Implementation phases

### Phase 1: classify the current failure as recoverable model-missing state

Add a deterministic classifier for:

- editor-authoritative document;
- no registered CRDT relay replica;
- no usable live model in the controller;
- disk intentionally non-authoritative.

Preflight/current-document resolution should attempt `ensure_document_model(file)`
before returning this as an error. Only fail closed if model
startup/reconciliation itself fails after a bounded attempt, and return that
root failure with recovery guidance.

Initial implementation status:

- `agent-doc-crdt-relay-io::ensure_document_model` performs the bounded
  read-only publish/re-observe loop without seeding a successful
  editor-authoritative read from disk.
- `agent-doc-document-realtime-io::try_resolve_current_doc_from_file` calls
  `observe_live_editor_authority_after_model_ensure`, so preflight/read
  current-authority checks get the recovery attempt before surfacing
  editor-authority failures. Write-side guards keep their existing force-disk
  behavior until the later lifecycle/commit-barrier phase moves those facts into
  durable backbone state.
- Remaining follow-up: fold editor attachment, relay model readiness, ensure
  outcome, and live model usability into durable backbone projections instead
  of deriving part of the classifier from process-local relay state.

Regression:

- SimWorld/live-style test where an editor-owned document has no relay replica
  at preflight start. Expected: model starts/reconciles, preflight proceeds or
  reports a bounded startup failure; the original "no registered replica" text
  is not surfaced as the final agent-facing contract.
- Current focused coverage: relay unit tests cover delayed registration success
  and bounded no-consumer failure; document-realtime IO covers the
  current-document resolver's final contract.

### Phase 2: move hot-path lifecycle facts into backbone state

Define typed lazily state for the facts currently inferred from sidecars:

- cycle phase and capture id;
- baseline/head/current hashes as advisory facts;
- editor attachment and live replica status;
- editor-sync/stale-disk state;
- response captured/applied/committed/session-check status;
- supervisor stale-binary/recycle-yield status.
- queue/supervisor hot-path gate state already migrated in the ledger above
  remains in typed facts/projections and must not regress to marker-file
  authority.

Read this state in preflight, write/finalize, repair, and session-check before
falling back to sidecar projections.

Initial implementation status:

- `agent-doc-cycle-state-io` emits the closeout facts listed above whenever the
  legacy cycle transition is accepted. This establishes a durable re-entry
  ledger for the existing phases.
- Semantic-merge acknowledgement notices now have typed closeout facts:
  `SemanticMergeAckRecorded` and `SemanticMergeAckCarriedForward`. The cycle
  JSON field is retained as a compatibility projection, while carry-forward and
  preflight warning output prefer the lazily closeout projection and only import
  a legacy JSON notice when the sidecar matches the projected cycle. Projection
  and `CycleState` fallback paths now use shared read/carry helpers for the
  one-cycle surfaced transform.
- Reader cutover in progress: session-check, repair, preflight, selected
  write-entry gates, and supervisor/Stop-hook gates query the closeout projection
  first for open/terminal phase decisions and only fall back to the JSON cycle
  sidecar when no matching projection exists. Session-check closeout
  signal/readiness guards, repair recovery gates, preflight completion /
  route-recovery gates, and idle-watch convergence gates now share that
  projection-aware view for phase and capture identity; richer
  backlog/pending/proof payloads and remaining preflight recovery payloads still
  read compatibility sidecar details until those fields move into typed backbone
  facts.
- Repair pending-response reader cutover is active: repair queries the
  pending-response projection first and only imports the file backup when the
  backbone has no pending/clear fact for the document. The state backbone
  projects clear state from `WriteApplied` / `PendingResponseCleared` facts even
  when the original pending capture event is absent, so stale pending files
  cannot override a durable clear and are retired as backup projections.
- Write/finalize committed-cycle gates now read the projection-aware cycle view
  for terminal/open phase decisions. Terminal closeout proof payloads are now
  typed `TerminalCloseoutProofRecorded` backbone facts, and closeout recovery
  evidence observations are typed `CloseoutRecoveryEvidenceRecorded` facts.
  Recovery decisions prefer the recovery-evidence projection when its visible
  markdown hash still matches the current file, and missing-response
  classification now consumes projected response-body evidence before the
  `stuck_captured_cycle` sidecar path. Snapshot/head and snapshot/visible drift
  classification also consumes projected drift evidence before compatibility
  snapshot reads. Fresh recovery evidence gathering and stuck-cycle detection
  now also prefer the captured-response projection before capture JSON for body
  proof, fresh queue-only drift proof uses projected response baseline hashes
  before capture JSON hashes, and write-runtime/converge response-body readers
  do the same for queue closeout and active-response-removal checks. Remaining
  write/finalize work is to make every detailed classifier and payload reader
  prefer those projections before compatibility sidecars.
- Closeout recovery diagnostics and command payloads now read the
  projection-aware cycle view before using cycle sidecar details. Projection-only
  open cycles produce `open_cycle` recovery and include the projected cycle id,
  phase, and capture id in the command payload. Remaining detailed-payload work
  is to replace the remaining compatibility-only snapshot/capture/hash evidence
  readers with the new typed recovery-evidence projection where those facts are
  the real authority.
- Repair preflight cancellation and stale-preflight repair now use a shared
  captured-response proof helper that reads the typed closeout projection before
  capture JSON compatibility files. A matching `ResponseCaptured` body
  projection, or projected capture id plus response hash, protects the cycle even
  when the capture sidecar is missing; stale empty-cycle repair only treats the
  cycle as empty after that projection-first proof check fails.
- Repair committed-capture materialization checks now read the projected
  committed captured-response body before `latest_committed` capture JSON. A
  committed closeout projection can prove the response body is already in HEAD
  even after the committed capture sidecar is missing; the JSON record remains
  the compatibility fallback for older projection gaps.
- Repair historical committed-capture replay now uses the same committed
  captured-response projection payload before `latest_committed` capture JSON.
  Orphaned-prompt replay can recover from a committed closeout projection after
  the committed capture sidecar is missing, while the legacy JSON record remains
  only the fallback when no matching projected response body exists.
- Session-check blocked-followup, gated-phase split, queue-audit
  partial-completion, pending, response, queue-head/provenance, partial-staging,
  write-time pending, and recoverable cache-conflict readers now use the
  projection-aware cycle view, and their captured-response body payloads now
  prefer the typed closeout projection before capture JSON compatibility
  fallback. Remaining cutover: replace detailed backlog/pending/hash/proof
  payload reads with typed backbone facts, then use cycle/capture/snapshot
  sidecars only as compatibility projections or recovery evidence.
- Commit-proof status: the cycle-state bridge still emits a compatibility
  content-hash `CommitObserved` when that layer lacks git context, but the git
  closeout path now appends the exact `HEAD` SHA after real and no-op closeout.
  `agent-doc-cycle-state-io::load_latest_terminal_closeout_proof` exposes the
  typed terminal proof projection for structured readers; closeout and CLI
  assertions now prefer that projection instead of parsing the proof ledger.
  Commit materialization checks now also prefer the typed captured-response
  projection before active capture JSON, and queue-consume commit seam response
  proof plus post-response hook metadata use the same projection before capture
  JSON fallback. Remaining cutover is to route any new production proof guard
  through terminal/captured-response projections instead of trusting terminal or
  capture sidecar state.

Each stage must have an explicit re-entry transition:

- `preflight_started` re-entry reconciles visible document, editor model, and
  backbone state before diffing.
- `response_captured` re-entry either reapplies the captured response, proves it
  is already visible/committed, or abandons it through a named obsolete-capture
  transition when a newer operator prompt superseded it.
- `write_applied` re-entry either reaches commit/session-check or rolls forward
  from the authoritative model; it does not ask sidecar baselines to decide
  authority.
- `committed` re-entry verifies HEAD/document/backbone agreement and refreshes
  projections.

Regression:

- A stale snapshot/cycle sidecar cannot block preflight when backbone state and
  visible document prove the current response is already committed or no
  response is owed.

### Phase 3: demote sidecars to projections

Sidecars remain useful for diagnostics, crash recovery, and audit history, but
their contract changes:

- Sidecar read/parse/mismatch failures are warnings unless no authoritative
  backbone/editor/git state exists.
- `reset --from-current --preserve-session` rebuilds projections from backbone
  + visible document state, not the other way around.
- `write --commit` repair validates against backbone lifecycle state and the
  visible document model first; sidecar baseline drift cannot wedge an otherwise
  recoverable closeout.
- Drift discovered on re-entry is reconciled inside agent-doc: classify the
  conflicting facts, update the backbone/projections to a single named state,
  and continue or fail closed with that named state. Do not surface raw
  filesystem projection mismatch as the recovery contract when a deterministic
  reconciliation exists.

Regression:

- Reproduce a stale `response_captured` sidecar with a visible document that has
  moved forward. Expected: repair either absorbs the already-visible response or
  abandons the obsolete capture through a named lifecycle transition; it does
  not loop on "captured response baseline no longer matches current document."

### Phase 4: state-vector commit barrier

Replace patch/receipt sidecar proof as the primary closeout barrier with state-vector
or equivalent editor-sync facts from the live model:

- every currently attached editor replica has flushed through the state vector
  the supervisor will snapshot;
- disconnected/stale editors are classified explicitly;
- write intent, patch id, expected response hash, editor patch-received event,
  applied/rejected outcome, and editor-visible buffer hash are folded into a
  bidirectional lazily projection; legacy ACK sidecars are not compatibility
  inputs and old ACK-only plugins are rejected with a version/install error;
- commit is a named rejected transition when editor-sync proof is missing.

This is the load-bearing version of the `#adstatechart3` closeout guard.

Regression:

- A stale editor buffer cannot be committed from disk.
- A missing/corrupt sidecar cannot block a commit when the editor-sync state
  vector proves the model is current.

## Acceptance

- The original `equityfundingsource.md` incident has a deterministic regression.
- Preflight no longer surfaces "editor authority but no registered CRDT replica"
  as a final agent-facing error when the model can be auto-started/reconciled.
- Snapshot/cycle sidecar drift is no longer hot-path authority.
- Every lifecycle stage has a deterministic crash/re-entry regression proving it
  rolls forward, proves completion, or reconciles drift before failing closed.
- `make check` passes in `src/agent-doc`.
- A live editor proof shows the document can recover from missing relay/model
  registration without forcing disk or asking the agent/operator to run
  sidecar-reset commands.
