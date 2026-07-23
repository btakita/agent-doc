# State Backbone

`ResponseCaptured` is content-bearing recovery authority: it retains the full response body and the full editor-visible baseline used at capture time, in addition to their hashes. This lets repair distinguish partial response fragments from operator text without consulting the working tree as authority. Legacy hash-only facts can be upgraded by a new revisioned fact after an independently hash-matching baseline is found. `DocumentWriteDeferred` retains the resulting reconciled target when no editor replica can publish it; only matching convergence clears that intent.

`agent-doc` keeps the Cycle State Machine as the global lifecycle authority for
one response turn. Other state must not be folded into that same finite state
machine. Document convergence, queue ownership, editor transport, route
readiness, supervisor health, and proof collection each have independent axes
that would make one global FSM brittle and hard to replay.

The durable backbone is:

1. A typed, append-only event ledger.
2. Deterministic projections derived from that ledger.
3. Single-owner actors for live mutable surfaces.
4. Local FSMs only for small closed subdomains.

## Cycle FSM Scope

The Cycle State Machine owns only the turn lifecycle:

- `preflight_started`
- `response_captured`
- `write_applied`
- `committed`
- interrupted or abandoned recovery states

The cycle FSM is the closeout gate. A response is not complete until the cycle
reaches `committed` and `session-check` can prove there is no unresolved prompt
or pending write boundary for that turn.

The cycle FSM must not directly encode queue ordering, editor receipt wire shapes,
supervisor restart policy, route prompt readiness, or proof-specific transport
facts. Those are inputs to the cycle closeout decision, not cycle phases.

## Event Ledger

Every meaningful state transition should have a typed fact that can be replayed.
Examples:

- `PreflightStarted`
- `BaselineSaved`
- `QueueHeadSelected`
- `QueueHeadCompleted`
- `QueueContextClearDeferred`
- `QueueContextClearStarted`
- `QueueContextClearSettled`
- `QueueDrainStallContinuationRecorded`
- `QueueDrainStallContinuationCleared`
- `ResponseCaptured`
- `WriteApplied`
- `EditorPatchApplied`
- `EditorPatchRejected`
- `IpcProofInsufficient`
- `VisibleWriteCommitCandidateObserved`
- `DocumentWriteDeferred`
- `DocumentWriteConverged`
- `CommitObserved`
- `SessionCheckPassed`
- `CycleAbandoned`
- `AgentRestartPerformed`
- `CapabilityProofObserved`
- `ActorGenerationChanged`
- `SupervisorRecycleStarted`
- `SupervisorRecycleSettled`
- `RouteSubmitStarted`
- `RouteSubmitSettled`
- `RouteSubmitBlocked`

Events must carry stable ids where available: document hash, session id, cycle
id, actor generation, patch id, queue node key, backlog id, and causation id.
The event log is append-only. Corrections are new events that supersede earlier
facts by projection rules; they are not in-place mutation of old facts.
Content-bearing visible-write receipts include the model revision in their
stable event identity so a fresh editor publication can supersede a legacy
hash-only fact for the same patch without losing idempotence. A visible-write
hash is validation metadata, not recoverable content authority.
`DocumentWriteDeferred` is likewise content-bearing: it retains both
`expected_content` and `target_content`. A successor intent must be based on the
prior target or component-compose the prior and new targets over the retained
base. Matching editor convergence alone retires the intent; a force-disk
projection or commit does not erase reconnect authority.
Its cause is a `DocumentWriteDeferredReason` enum in facts and projections, not
free text. CRDT convergence phases and replica wake-event kinds follow the same
rule: stable snake-case strings are serialization/log tokens at the boundary,
while state transitions and dispatch use exhaustive variants internally.

Implementation: `agent-doc-state-backbone/src/lib.rs` defines
`StateEvent`, `StateFact`, and `EventLedger`. The ledger deduplicates event ids
during projection replay so duplicate delivery stays idempotent, while
`causation_id` preserves the chain from prompt, queue head, IPC patch, route
dispatch, or proof marker to the emitted fact.

Every durable event also has a monotonic per-document `document_version`.
Editor replay acknowledgements are collected in `state_event_peer_acks`, keyed
by `(document_hash, peer_key)`, where `peer_key` is derived from the live
PID-scoped editor registration `(pid, editor_id)`. A state subscription returns
the durable version represented by its snapshot/delta. The editor records that
version only after the graph message applies successfully and reports it on the
next subscription; the controller accepts it only while the exact registration
is live. Acknowledgements advance monotonically and a peer cannot acknowledge a
version above the document's durable high-water mark.

This acknowledgement table is collection-only for now. Existing fact-specific
count caps remain the retention authority and do not consult peer cursors. A
future retention change may delete below `MIN(acked_version)` across live peers
only after it also defines deterministic crashed-peer eviction; until both
rules land together, a missing or stale acknowledgement is never grounds for
additional deletion.

## Projections

Current state is derived by deterministic reducers over the event ledger and the
document components. Required projections include:

| Projection | Owns |
|---|---|
| Document projection | current component bodies, boundaries, snapshot relation, prompt-bearing tail, editor attachment, CRDT relay model/replica status, document-model ensure outcome |
| Queue projection | active head, struck/completed heads, drainability, backlog-to-queue sync, context-clear phase |
| Closeout projection | latest cycle phase, captured response materialization, pending write boundary |
| Transport projection | PID-scoped endpoint, named binary intent, expected generation/hash, editor accepted/visible/rejected facts, retry/backoff state |
| Supervisor projection | actor state, child pid, harness, capability proof, restart/recycle epoch |
| Route projection | authoritative pane, readiness, dispatch authorization, route-submit phase, dispatch proof |
| Proof projection | ops-log markers, typed verify/disproof predicates, semantic completion advisories |

Reducers must be idempotent and replay-testable. If two modules need the same
answer, they should consume the same projection instead of recomputing from
free-form logs or partial document text.

The document projection retains the full target content for an admitted write
that cannot reach a live editor replica. Only `DocumentWriteConverged` for the
matching intent clears it. Unrelated convergence facts must not erase pending
work, and a zero-member relay acknowledgement must not manufacture delivery or
disk authority.

Implementation: `StateBackboneProjection` reduces the event ledger into
document, queue, closeout, transport, supervisor, route, and proof projections.
The closeout projection delegates phase advancement to the existing
`CyclePhaseMachine`; the other projections use their own small state machines
for closed subdomains.

`agent-doc-cycle-state-io` appends `PreflightStarted`, `ResponseCaptured`,
`WriteApplied`, `CommitObserved`, and `CycleAbandoned` facts with stable event
ids after accepted lifecycle transitions. A transition must not be reported as
successful if the matching `state.db` fact cannot be recorded. Preflight,
session-check, routing, repair, and closeout all reduce the same closeout
projection for open/terminal authority; no filesystem cycle record is read or
replayed.

The git closeout layer also appends `CommitObserved` with the exact `HEAD` SHA
after a successful real commit or an already-current no-op closeout. That fact
is the authoritative commit identity.

## Editor Projection Bridge

Editor integrations must treat the FFI state backbone as the shared foundation.
JetBrains, VS Code, and later editors should:

- bind `agent_doc_state_projection` and `agent_doc_record_state_event` when the
  native library exposes them;
- compute the canonical document hash with the same canonical-path SHA-256 key
  used by snapshots;
- report editor transport observations using the shared `EditorIntent` and
  lifecycle vocabulary (`IntentCaptured`, `CanonicalApplied`,
  `ReplicaAccepted`, `ReplicaVisible`, `DiskProjected`, `Committed`);
- report route dispatch observations as route-owner generation plus
  readiness/proof events instead of relying only on plugin-local booleans; and
- render route, transport, and proof status from `DocumentStateProjection`
  slices where status/log output needs current state.

The editor bridge may keep small in-process counters for owner generations, but
it must not implement a second route, IPC, or proof FSM. If an installed plugin
or native library cannot publish lazily transport receipts, it is incompatible:
install/reload must surface a version error instead of falling back to
ACK-shaped proof.

Package-level bridge parity is explicit:

- `lazily-kt` and `lazily-js` own reusable state-projection clients and pure
  helper contracts for canonical document hashing, `StateEvent` JSON,
  projection summary rendering, and pointer/free lifecycle.
- JetBrains keeps a plugin-local canonical bridge instead of depending directly
  on `lazily-kt`, because the plugin build is constrained by the IntelliJ
  Kotlin/JBR toolchain while `lazily-kt` is a standalone Kotlin/JVM package.
- VS Code keeps a plugin-local canonical bridge instead of importing
  `@lazily/js` at runtime, because the extension is CommonJS-packaged while
  `@lazily/js` is ESM. VS Code tests must compare its pure helpers against the
  package helpers so this duplicate adapter cannot silently drift.
- Rust owns the authoritative `ProjectionSummary` compact string. Editor
  compact-summary helpers must keep matching `route=<readiness> pane=<pane>
  transport=<intent>:<phase> proof_markers=<count>`, and cross-language coverage
  must drive both plugins through queued/retry/accepted/visible transitions plus
  route started/proven/blocked events.

## State Wire (lazily-spec snapshot/delta)

`#lazilystatesync2` exposes the projection as a reactive lazily-spec wire graph
so plugins mirror it into a lazily-kt / lazily-js graph and apply deltas instead
of re-rendering the full snapshot on every event.

Implementation: `agent-doc-orchestration/src/state_wire.rs` maps
`DocumentStateProjection` onto the `src/lazily-spec/schemas/{snapshot,delta}.json`
envelope. The existing `agent_doc_state_projection` FFI (full
`DocumentStateProjection` JSON) stays as the cold round-trip path and is
unchanged.

### FFI surface

- `agent_doc_state_subscribe(document_hash, last_epoch) -> JSON` — returns a
  lazily-spec message with a `"type"` discriminator:
  - `"snapshot"` when `last_epoch == 0` (cold read) or the document has no
    accepted events yet — a full graph image the mirror applies once.
  - `"delta"` when `0 < last_epoch < current_epoch` — ordered `ops` the mirror
    applies verbatim to converge from `last_epoch` to current.
  - `"delta"` with empty `ops` when the caller is already current.

### type_tag vocabulary

Each projection node maps to one lazily-spec node with a stable `type_tag`.
`slot_id = fnv1a(document_hash, type_tag, entity_key)` so Rust/Kotlin/JS address
the same node without a central allocator (FNV-1a is re-implemented identically
across the three languages — no platform `Hasher` drift).

| type_tag | entity_key | source |
|---|---|---|
| `agent_doc.document.baseline` | document_hash | `BaselineProjection` |
| `agent_doc.queue` | document_hash | `QueueProjection` (singleton) |
| `agent_doc.queue.head` | node_key | `QueueHeadProjection` |
| `agent_doc.closeout.cycle` | cycle_id | `CloseoutProjection` |
| `agent_doc.transport.patch` | patch_id | `TransportPatchProjection` |
| `agent_doc.supervisor.owner` | owner name | `OwnerProjection` |
| `agent_doc.route` | document_hash | `RouteProjection` (singleton, including `RouteSubmitProjection`) |
| `agent_doc.proof.marker` | marker | `ProofMarkerProjection` |

Node payloads are `base64(serde_json(struct))`.

While a cycle is open, its closeout payload may include `realtime_steering`.
`RealtimeSteeringObserved` facts replace that aggregate using canonical CRDT
content-hash event identity, so warm subscribers receive an ordinary `cell_set`
delta. The payload carries the primary steering kind, total directive count,
preview, and full ordered verbatim aggregate. `PreflightStarted`, commit, and
abandonment clear the field at cycle boundaries.

### Derivation edges

The snapshot/delta carry dependency edges so a plugin mirror can
invalidate/recompute only a derived subtree instead of re-rendering the whole
projection:

- `closeout.cycle → document.baseline`
- `queue.head → closeout.cycle`
- `transport.patch → closeout.cycle`
- `route → supervisor.owner[route_dispatch]`
- `transport.patch → supervisor.owner[editor_ipc_bridge]`

### Epoch + idempotence

- lazily-spec `epoch` = per-document monotonic counter = number of accepted
  (deduped) `StateEvent`s targeting the document (`EventLedger::document_epoch`).
  A re-emit/replay does not bump the epoch.
- The projection is a pure fold of deduped events, so delta application is
  deterministic and idempotent — a re-emit yields a no-op (empty) delta. This is
  the property `#queuestatemachine` / `#qdedupsync` build on.
- The ledger is append-only within a process lifetime, so any
  `last_epoch <= current_epoch` is satisfiable without a resync. Deltas may span
  multiple epochs (`epoch > base_epoch + 1`); the ordered `ops` converge
  identically to a fresh snapshot.

The `type_tag` table is the in-repo producer half of the wire vocabulary. The
canonical schema pin (`lazily-spec/schemas/agent-doc-state.json` + a conformance
snapshot/delta pair) is `#lazilyspecpin`, a sibling `lazily-spec` change.

## Actor Ownership


Live mutable surfaces have exactly one current owner:

- document writer
- editor IPC bridge
- route/dispatch actor
- supervisor/child process actor
- queue orchestrator

Actors communicate by events, leases, epochs, and explicit proofs. A stale actor
may report facts, but projections must reject reports whose generation or epoch
does not match the current owner. This is why restart and capability proof code
uses epochs rather than trusting any later-arriving thread result.

Implementation: owner reports carry a `StateOwner` and generation. The backbone
projection records rejected stale events instead of applying late reports from
an old editor IPC bridge, route dispatcher, supervisor, queue orchestrator, or
document writer.

## Where Other Models Fit

| Model | Fit | Use |
|---|---|---|
| FSM | Strong local fit | small closed state sets: cycle phase, transport connection, proof gate, recycle lifecycle |
| Behavior tree | Policy helper only | inspectable recovery/dispatch priority and fallback logic that reads projections and emits commands |
| GOAP | Poor durable-state fit | avoid for correctness-critical closeout; planning machinery is harder to replay than reducers |
| Coroutine | Good protocol expression | linear handshakes such as writeback, startup proof wait, or clean-exit restart; persist checkpoints as events |
| Event-driven | Strong backbone fit when typed | append-only events, causation ids, idempotent reducers, replay tests |
| MPC | Not a fit | agent-doc state is discrete workflow convergence, not continuous control optimization |

Implemented local FSMs:

| FSM | Domain | Purpose |
|---|---|---|
| `CyclePhaseMachine` | closeout | existing turn lifecycle authority |
| `QueueHeadMachine` | queue | pending, selected, deferred, completed head lifecycle |
| `QueueContextClearMachine` | queue | deferred operator clear, explicit context-clear in-flight, and settled window |
| `QueueDrainStallMachine` | queue | one-shot continuation-pending stall signal versus reconciled/cleared |
| `TransportPatchMachine` | transport | queued IPC patch, applied/rejected receipt, insufficient proof, retry, force-disk fallback |
| `ActorLifecycleMachine` | supervisor/owner | starting, ready, busy, waiting-input, restarting, stale, closed |
| `SupervisorRecycleMachine` | supervisor/recycle | in-flight versus settled supervisor recycle gates |
| `RouteReadinessMachine` | route | pane observed through dispatch proof |
| `RouteSubmitMachine` | route | idle, in-flight, and bounded blocked submit windows |
| `ProofGateMachine` | proof | marker observed versus disproved |

## Regression Rule

New state-bearing behavior must answer two questions in the same change:

- Which owner emits the typed event?
- Which projection reduces it into current state?

Adding another tactical `reason=...`, `proof=...`, ad hoc boolean, or log-only
string in a hot path is not enough unless the change also explains why that fact
is intentionally not part of the state backbone.
