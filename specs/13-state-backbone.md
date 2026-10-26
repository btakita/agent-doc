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
- `EditorOpCaptureCheckpointed`
- `EditorOpCaptureCleared`
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

`SupervisorRecycleSettled` carries the epoch it was minted from, and the
projection drops a settle below its current recycle epoch as stale. A
supervisor's `watch_loop_started` settle can therefore be minted from a read
that has not yet seen its own `SupervisorRecycleStarted` (observed during an
install-time controller handoff: `started-N+1` followed by `settled-N`), which
would pin the document `InFlight` forever and make every dispatch-only reopen
refuse as "mid-recycle". The settle handler verifies its fact landed: when the
projection read back is still unsettled at a strictly newer epoch than the one
minted, it re-mints `settled-<outstanding epoch>` (bounded, logged as
`supervisor_recycle_settle_reminted`). This is sound because `Started` for a
document is published only by that document's supervisor immediately before
its own `execve`, and the settling process is what that `execve` produced. The
stale row stays in the append-only log; the covering row after it settles the
recycle on replay as well as live (`#fixruninfra`).

The re-mint verifies against the controller's *live* projection, so that
projection must never regress below a fact the controller already applied. The
controller's durable reload (`refresh_memory`, run after external appends and
before serving editor state) loads off every lock; an append that commits and
applies during the load would otherwise be erased when the stale snapshot is
swapped in (observed 2026-10-10 on `frontend.md`: `started-4351` applied, a
racing reload restored `Requested@4350`, the settle minted `settled-4350`
against it and read back `Settled`, and the ledger replayed `InFlight@4351`
forever). Every controller-owned append+apply therefore holds the state-event
ingress lock and bumps an apply generation; the reload publishes its swap under
that lock (`#runfrontenddispatch`).

The full ledger replay never runs under that lock (`#refreshmemlockstall`). The
unlocked load records the highest `state_events.id` it folded; under the lock
the reload folds only the rows committed after that id, in id order, then swaps.
SQLite serializes writers and `id` is `AUTOINCREMENT`, so snapshot plus delta
is exactly a full replay at that instant, and an append that landed during the
load survives the swap. When the apply generation moved during the load the
reload logs `controller_refresh_memory_caught_up_after_concurrent_apply
rows=<n>` and re-reads the (small) actor store under the lock. The earlier
repair redid the *whole* load under the lock whenever the generation moved; on
a 215MB, 80k-event ledger that held ingress 1-4s (`state_event_ingress_slow
site=refresh_memory fact=durable_reload`), and an editor typing burst makes
that race the common case, so every editor-op append queued behind it and
exceeded its 250ms budget (sdk.md, 2026-10-11). The ingress hold is now bounded
by the delta, not the ledger.

Both repairs above prevent a new wedge; neither un-wedges a document whose
ledger already ends `InFlight` with no covering settle, and the dispatch-only
recycle gate kept refusing those as `recycle_ttl_elapsed_supervisor_alive`
(`#netadv5` R9) for as long as the supervisor lived. The gate therefore accepts
direct evidence that the recycle is over (`#dispatchreadyselfheal`):

- A supervisor `register_supervisor` with `state=ready` stamps
  `supervisor_leases.ready_registered_at`. Heartbeats never touch it: a
  heartbeat can come from a process about to `execve`, but a registration is
  only made by a process that has just started or re-entered after its
  `execve`.
- `recycle_inflight_unsettled_verdict_with_readiness` returns
  `ProceedReadyAfterStart` when that stamp is *strictly* greater than the
  `InFlight` mark's `marked_secs` (same-second is not evidence; an unstamped
  mark stays `FailClosed`). It takes precedence over the TTL and liveness
  verdicts; without the stamp the R9 refusal is unchanged.
- On that verdict the gate calls `supervisor_recycle_settle_ready_after_start`
  with the epoch it observed. The controller re-verifies against its live
  projection and the durable stamp and mints `settled-<that epoch>` (reason
  `dispatch_ready_after_start`) only when the projection is still `InFlight` at
  exactly that epoch. It never re-mints forward: a newer `Started` racing the
  request is a real pre-`execve` boundary, and a settle below its epoch is
  dropped by the reducer. A decline returns the projection unchanged and the
  gate re-classifies without the evidence; a newer epoch re-keys the wait. An
  unreachable controller does not refuse, because the stamp alone proves the
  boundary passed and the next dispatch retries the re-mint.

The readiness stamp is written by the controller when a supervisor *registers*,
so it only exists for registrations made after `#dispatchreadyselfheal`
shipped. A supervisor that registered `ready` under an older build never
re-registers while it lives, and the same `frontend.md` ledger refused again
after 0.35.483 (observed 2026-10-10 18:15:38: `InFlight@4351` still from
02:00:14, owner pid 1989646 on 0.35.481 with a deleted exe,
`ready_registered_at` NULL, R9 refusal; the controller's own stale-supervisor
replacement reaped it at 18:15:45 and the next dispatch proceeded). That
owner can never end the refusal itself: it will not stamp, and its own next
self-recycle publishes a *new* epoch rather than settling this one. The gate
therefore refines R9 by the owner's binary freshness (`#runfrontenderror`):

- `recycle_inflight_unsettled_verdict_with_evidence` returns
  `ReplaceSupersededOwner` only where the verdict would otherwise be
  `RefuseOwnerStillRecycling` *and* the live owner's `/proc/<pid>/exe` is
  stale against the installed binary (`host_supervisor_pid_binary_is_stale`).
  Unknown freshness or a current binary keeps the R9 refusal; inside the TTL
  a superseded owner may be the one mid-`execve`, so it keeps waiting.
- On that verdict the gate requests the owner's turn-safe stale-supervisor
  recycle once (`recycle_stale_supervisor_for_turn_stage`, stage
  `dispatch_only_recycle_gate`; it recycles at the owner's next safe
  boundary and never interrupts a live turn) and keeps re-arming the settle
  wait for at most `RECYCLE_SUPERSEDED_OWNER_REPLACEMENT_WAIT_SECS` (45s,
  `AGENT_DOC_RECYCLE_SUPERSEDED_OWNER_WAIT_SECS`), measured from the request,
  never from the hours-old mark. It still never injects while that owner is
  alive: it proceeds only through the existing verdicts (owner gone ⇒
  `ProceedAbandoned`, successor registered ⇒ `ProceedReadyAfterStart`, a
  settle ⇒ settled).
- If the bound elapses with the superseded owner still alive, the gate
  refuses with `refusal=superseded_owner_not_replaced` and
  `unblocker=restart_stale_supervisor`, naming the pid, instead of
  `wait_for_supervisor_recycle_settle`, which that process cannot satisfy.

The supervisor also stops creating this shape: an in-place `execve` that
returns (`Refused` or an IO error) after `supervisor_recycle_started` was
published never crossed the boundary, and the process keeps serving its
current binary, so it settles the epoch itself (reason `reexec_aborted`,
`supervisor_recycle_settled_after_aborted_reexec`) instead of leaving a live
owner pinned `InFlight` past the TTL.

Events must carry stable ids where available: document hash, session id, cycle
id, actor generation, patch id, queue node key, backlog id, and causation id.
The event log is append-only on the write path. Corrections are new events that
supersede earlier facts by projection rules; they are not in-place mutation of
old facts. Retention is the sole deletion path and follows the live-peer
watermark and fact-specific rules below.
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

The long-lived project controller does not retain that durable ledger in
memory. SQLite's unique `event_id` index deduplicates ingress; the controller
folds each accepted row into a compact current projection with a per-document
durable epoch, then discards the event payload and id. A subscriber at the
current epoch receives an empty delta. A cold or lagging subscriber receives a
current snapshot at the durable epoch, so process RSS is bounded by current
document state rather than controller age (GH #141).

Every durable event also has a monotonic per-document `document_version`.
Editor replay acknowledgements are collected in `state_event_peer_acks`, keyed
by `(document_hash, peer_key)`, where `peer_key` is derived from the live
PID-scoped editor registration `(pid, editor_id)`. A state subscription returns
the durable version represented by its snapshot/delta. The editor records that
version only after the graph message applies successfully and reports it on the
next subscription; the controller accepts it only while the exact registration
is live. Acknowledgements advance monotonically and a peer cannot acknowledge a
version above the document's durable high-water mark.

Retention deletes ledger rows strictly below `MIN(acked_version)` across the
controller's exact live PID-scoped registrations. A live registration without
an acknowledgement contributes zero and pins deletion. Ack rows for
registrations absent from that authoritative live set are evicted atomically
before the minimum is calculated; the OS-exit watcher and cold-start liveness
reconciliation therefore prevent a crashed IDE from pinning the watermark
forever. The exit path re-derives retention for every document the dead PID
holds open with a registration, read independently of the PID's alive state:
the durable `Alive{false}` is visible to in-process journal readers before the
exit folds it into the liveness plane, so the eviction must not depend on the
exit being the first path to observe the death. With no live registrations there is no peer watermark and this path
deletes no events. The row at the minimum is retained as the monotonic
per-document high-water anchor, and existing fact-specific count caps remain a
bounded-storage backstop. Eligible rows are deleted in bounded batches inside
the same acknowledgement transaction, so a large ledger cannot require one
unbounded SQL delete while the exact cursor anchor and durable high-water mark
remain intact.

## Projections

Current state is derived by deterministic reducers over the event ledger and the
document components. Required projections include:

| Projection | Owns |
|---|---|
| Document projection | current component bodies, boundaries, snapshot relation, prompt-bearing tail, editor attachment, CRDT relay model/replica status, captured editor-op epoch, document-model ensure outcome |
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
- Standalone `EventLedger` replay remains append-only and can derive a delta for
  any retained epoch. The controller's compact hot projection intentionally
  resynchronizes lagging subscribers with a snapshot instead of retaining every
  historical event in process memory. Deltas may span multiple epochs when
  history is available; either response converges identically to a fresh
  snapshot.

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

Queue item completion is a semantic lifecycle transition, not a checkbox parse
result. A proven closeout appends `QueueHeadCompleted`; `Completed` is terminal
within its hosting epoch, so delayed Markdown projection, polling, or a stale
`Selected` observation cannot reactivate the item. Queue/backlog/done text
projects that lifecycle and may be reconciled lazily after the fact is durable.

## State Propagation, Not Effect Inference

Every consequence (tmux pane order, focus, queue dispatch, editor delivery) is a
pure derivation of propagated state. The surface that can observe a state
publishes it as a Source, and the owning graph derives the consequence from
that Source's current value. Effects only *apply* a derived value; they never
decide it.

- **Publish the state, not a hint about it.** An editor publishes its actual
  layout: split columns in geometric order, plus the visible and selected
  documents in each. Downstream code must not reconstruct that layout from an
  event sequence, for example "editor removed, then reopened with
  `reuseOpen=false`, so treat it as a reorder". It must not reconstruct it from
  a previous fold's memory either. An event can trigger a re-publish of state,
  but it is never itself evidence of the state.
- **The current Source alone determines the derivation.** When the true state
  is observable, the derived value must not depend on how it was reached. Two
  states with the same content derive the same consequence: `[A | B]` followed
  by `[B | A]` yields tmux order `B, A` with no event history involved.
  Carry-over memory, such as the order-preserving fold in
  `RemoteLayoutMemory`, is allowed only for state the publisher marks as
  unknown.
- **Label unknown state; never guess it.** If a publisher cannot observe part
  of the state, it publishes that part as explicitly unknown (for example
  `geometry=unknown` for a JetBrains Remote Dev backend that cannot see the
  client's split geometry). The consumer then keeps its last derived value or
  fails closed, and the spec for that surface names the limit and the manual
  remedy. A plausible guess presented as state is a defect, even when it
  happens to be right. When another process does observe that state, it is
  that process's job to publish it: the JetBrains Client's 262/263 frontend
  module measures its editor windows (surface snapshot schema 2), and the
  backend forwards the derived columns as `column_order=editor` (GH #320
  option B). The `unknown` label remains only where no such publisher exists.
- **Ban structural guessing.** Pane membership and order change only on a
  generation-fenced authoritative frontend layout snapshot (or another
  explicitly named structural authority). Selection-only, partial, unknown,
  and timing-ordered input may update focus intent, but must preserve the last
  authoritative frontend vector or defer. There is no provisional structural
  state and no backend slot-replacement inference (GH #343).
- **A connected frontend is the editor-layout source of truth.** Its typed
  column vector is copied mechanically into the desired tmux effect target;
  the backend/controller does not own a second projected editor layout, fold
  it against retained backend state, or reorder/reconstruct it. The only
  backend states are the cached generation-fenced frontend fact, the identical
  desired tmux target, and observed tmux receipt/convergence. Cached authority
  exists for idempotence/reconnect, not as a backend-authored layout.
- **A manual order assertion is a bounded override, not inferred state.** Both
  editor plugins submit `Sync Tmux Layout` as `caller_kind=manual`. If that
  command carries `column_order=unknown`, the controller adopts its submitted
  columns only when the retained basis has no positive order authority. The
  vector remains labelled `unknown` and stabilizes later automatic folds; it
  cannot replace measured editor geometry. Tmux observation already belongs to
  the controller, so this repair requires neither editor polling nor another
  lazy state-projection channel (GH #338).
- **Do not make layout authority a bidirectional echo.** The flow is
  owner-directed: editors publish editor-surface facts to the controller, while
  the controller observes tmux locally and may publish typed layout status back
  for presentation and acknowledgement. That status is not editor-layout input
  and must never be echoed back as a new desired surface. Consequently the
  controller does not guess tmux state, and it does not need an editor round
  trip to reconcile it. When no process can observe editor split geometry, the
  order remains explicitly `unknown`; a manual assertion supplies the bounded
  missing intent until a positive editor-geometry Source appears (GH #338).
- **Never drop observed state on a local precondition.** A publisher must not
  discard state it did observe because some other local view is empty, as in
  GH #320, where the remote-client visible editors were dropped because the
  backend's `selectedFiles` was empty. It publishes what it has and labels
  the rest unknown.
- **Prefer the reactive path over shelling out to the CLI** for state that a
  long-lived surface already holds. Re-deriving editor state per command, for
  example `--col` arguments on a CLI route, is a snapshot of the state, not
  propagation. It must reach the same derivation and must not become a second
  source of truth. JetBrains `Run Agent Doc` and `Sync Tmux Layout` follow this
  through `surface_layout_reference_v1`: they publish the current surface on the
  ordinary observation path and name that publication, and the controller
  derives their layout from its surface Source (`specs/08-session-routing.md`,
  "Editor commands name the propagated surface"). The plugin's `--col` snapshot
  survives only as a logged fallback for a controller without the capability.
- **Frontend ingress completion is not authority retirement.** The Remote Dev
  `openIngress` flow is an external event boundary. A normal completion that
  emitted no lease is absence of frontend-authority evidence, not proof that a
  loaded frontend module is unavailable. The frontend retries that subscription
  with bounded exponential backoff, records whether a lease was observed, resets
  the empty-completion streak when one arrives, and lets coroutine cancellation
  terminate the retry. A lease event drives the existing snapshot publisher;
  only an explicit generation-fenced retire event may release backend authority
  and enable selection fallback (GH #367).
- **Authenticated frontend ingress is not by itself pane-layout authority.** The
  modular frontend also opens the snapshot protocol in a monolithic IDE. The
  authority transition is enabled only when the authenticated client id names a
  remote platform session (`ClientType.isRemote`). A local lease may continue to
  carry the atomic editor-view snapshot, but `layoutAuthorityActive` remains
  false, the classic editor-surface frame remains live, and no Remote Dev layout
  endpoint may publish or retire. This guard must precede the handoff: filtering
  only the later forward creates a no-publisher state in which the classic source
  is retired and the local snapshot is rejected as `local_client`.
  The classic source must also remain *installed* in that package: a modular
  backend lifecycle that omits the classic selection/focus/activation edges is the
  same no-publisher state reached by a different route, because nothing then
  publishes a local tab switch until an unrelated republish (native-library
  reload, Run Agent Doc, container change).
- **Command applicability is derived from the retained Source's authority.** A
  route may consume flat Remote Dev evidence as a non-structural `ensure`
  request. An exact `Sync Tmux Panes` command requires measured columns; after
  its flat publication is retained, it finishes as an idempotent preserve
  terminal instead of submitting an unauthorized structural effect or turning
  the expected absence of geometry into a user-facing failure. The pure
  `SurfaceCommandReference` decision owns this table; action callers only apply
  its typed result (`Reference`, `PreserveRetainedLayout`, or rollout fallback).

## Regression Rule

New state-bearing behavior must answer three questions in the same change:

- Which owner emits the typed event?
- Which projection reduces it into current state?
- Is every consequence derived from propagated state, rather than inferred from
  an event sequence or an effect's history (see State Propagation, Not Effect
  Inference)?

Adding another tactical `reason=...`, `proof=...`, ad hoc boolean, or log-only
string in a hot path is not enough unless the change also explains why that fact
is intentionally not part of the state backbone.
