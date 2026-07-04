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
- The state backbone owns the current facts: editor attachment, CRDT replica
  registration, editor-sync status, lifecycle phase, capture/recovery state,
  commit barrier readiness, and supervisor staleness.
- Every pipeline stage is reentrant. If the process crashes after any durable
  transition, restarting the same stage must either complete the transition,
  prove it already completed, or reconcile the drift into a named backbone state
  before continuing.
- Snapshot, ack-content, live-buffer, and cycle-state sidecars become derived
  audit/recovery projections. A stale/missing sidecar can warn or assist repair,
  but it cannot be the reason preflight exposes an unreconciled authority error
  when the model can be started or rebuilt.
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

## Implementation phases

### Phase 1: classify the current failure as recoverable model-missing state

Add a deterministic classifier for:

- editor-authoritative document;
- no registered CRDT relay replica;
- no usable live model in the controller;
- disk intentionally non-authoritative.

Preflight should attempt `ensure_document_model(file)` before returning this as
an error. Only fail closed if model startup/reconciliation itself fails after a
bounded attempt, and return that root failure with recovery guidance.

Regression:

- SimWorld/live-style test where an editor-owned document has no relay replica
  at preflight start. Expected: model starts/reconciles, preflight proceeds or
  reports a bounded startup failure; the original "no registered replica" text
  is not surfaced as the final agent-facing contract.

### Phase 2: move hot-path lifecycle facts into backbone state

Define typed lazily state for the facts currently inferred from sidecars:

- cycle phase and capture id;
- baseline/head/current hashes as advisory facts;
- editor attachment and live replica status;
- editor-sync/stale-disk state;
- response captured/applied/committed/session-check status;
- supervisor stale-binary/recycle-yield status.

Read this state in preflight, write/finalize, repair, and session-check before
falling back to sidecar projections.

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

Replace patch/ack-content proof as the primary closeout barrier with state-vector
or equivalent editor-sync facts from the live model:

- every currently attached editor replica has flushed through the state vector
  the supervisor will snapshot;
- disconnected/stale editors are classified explicitly;
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
