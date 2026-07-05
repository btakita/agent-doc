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
  reused across parallel graphs. Agent-doc should adopt that in two explicit
  surfaces next: `CycleContext` (rename/replace the short-lived `RunContext`
  cycle IO graph) and `RealtimeDocumentContext` (a typed
  `ThreadSafeContext`-backed realtime document graph for cross-thread editor /
  watcher / actor authority). This checkpoint only lands the lazily primitive
  and CurrentDocument-backed cache; it does not claim that the rename/adoption is
  complete.
- Cutover progress: `CycleContext` and `ActorContext` now alias direct
  `lazily::TypedContext` schemas, with memoized Slot/Cell factories living in
  `agent-doc-run-context-io`. The old `DocumentContext` wrapper is removed, and
  callers use the typed contexts plus `AgentDocContextExt` while the remaining
  realtime document graph is still being collapsed into direct lazily contexts.
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
  late-IPC overapplication / unstarted prompt-bearing detectors use it. Remaining
  direct document-I/O clusters are still load-bearing in commit, write,
  write-converge, repair, preflight-io maintenance, session-check guard modules,
  queue/backlog command IO, run/stream/compact adapters, sync/route/start helpers,
  and command-only utilities; each must be classified as either realtime-current
  document access, explicit detached-disk/audit access, or sidecar/metadata IO.
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
  snapshot reads. Remaining write/finalize work is to make every detailed
  classifier and payload reader prefer those projections before compatibility
  sidecars.
- Closeout recovery diagnostics and command payloads now read the
  projection-aware cycle view before using cycle sidecar details. Projection-only
  open cycles produce `open_cycle` recovery and include the projected cycle id,
  phase, and capture id in the command payload. Remaining detailed-payload work
  is to replace the remaining compatibility-only snapshot/capture/hash evidence
  readers with the new typed recovery-evidence projection where those facts are
  the real authority.
- Session-check blocked-followup, gated-phase split, queue-audit
  partial-completion, pending, response, queue-head/provenance, partial-staging,
  write-time pending, and recoverable cache-conflict readers now use the
  projection-aware cycle view. Remaining cutover: replace detailed
  backlog/pending/hash/proof payload reads with typed backbone facts, then use
  cycle/capture/snapshot sidecars only as compatibility projections or recovery
  evidence.
- Commit-proof status: the cycle-state bridge still emits a compatibility
  content-hash `CommitObserved` when that layer lacks git context, but the git
  closeout path now appends the exact `HEAD` SHA after real and no-op closeout.
  `agent-doc-cycle-state-io::load_latest_terminal_closeout_proof` exposes the
  typed terminal proof projection for structured readers; closeout and CLI
  assertions now prefer that projection instead of parsing the proof ledger.
  Remaining cutover is to route any new production proof guard through that
  projection proof instead of trusting terminal sidecar state.

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
