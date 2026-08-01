# Plan — controller-owned editor reactive plane and SQLite boundary

Status: implemented and verified

## Incident and root cause

An IntelliJ process crashed with `SIGBUS/BUS_ADRERR` in SQLite
`walFindFrame`. The faulting address was inside a deleted
`.agent-doc/state.db-shm` mapping owned by the freshly hot-reloaded
`libagent_doc` generation. The native backtrace was:

```text
editor surface observation
  -> local Lazily effect
  -> run_intent_via_controller
  -> enqueue_tmux_layout_sync
  -> connect_or_launch
  -> status
  -> read_bootstrap
  -> open_state_db
  -> SQLite WAL read
```

The editor did not need durable state. A controller client helper leaked
actorless bootstrap and SQLite access into the embedded editor process.
Native reload made that leak fatal by allowing an editor-owned SQLite mapping
to overlap the database/WAL lifecycle of other processes.

## Architecture contract

**Invariant:** During normal runtime, only the Project Controller process may
open the project's `.agent-doc/state.db`; editor processes publish ephemeral
facts and consume projections without opening, mapping, replacing, or repairing
the database.

**Policy owner:** A Project Controller `ProcessScope` owns editor-surface
observations, derived layout/focus intent, document-authority subscriptions,
effect receipts, and the durable sink. Editor adapters own only collection of
IDE facts and display of controller projections.

**Allowed SQLite exceptions:** Explicit offline administration, migration, and
repair commands may open the database only after proving that the Project
Controller and other writers are stopped. Cold startup hydration occurs inside
the controller before its live process scope accepts ingress.

### Transition table

| Input | Controller decision | Editor-visible projection |
|---|---|---|
| New generation-tagged surface observation | Publish the surface `Source`; recompute intent | Accepted with observation revision |
| Duplicate observation | Preserve graph state; run no effect | Idle/already-observed |
| Stale client generation or sequence | Reject without mutating graph state | Stale |
| Surface and tmux observations agree | Derive `Idle` | Idle |
| Focus differs while layout agrees | Derive `Focus`; run idempotent focus effect | Pending, then applied/failed receipt |
| Layout differs and passive sync is safe | Derive `Sync`; run generation-fenced sync effect | Pending, then applied/preserved/failed receipt |
| Controller unavailable | Do not launch, query SQLite, or infer drift | Unavailable; retry only from later ingress/reconnect |
| Client disconnects or reloads | Retire that client's membership generation | Disconnected; no native state survives reload |
| Explicit operator action | Execute the named imperative RPC | Request/response result |
| Durable sink fails | Keep live projection authoritative and mark sink backpressure | Degraded durability, live state unchanged |

### Evidence inputs

- `EditorSurfaceObservation`: project identity, editor client identity, client
  generation, monotone sequence, focused document, open/visible documents, and
  column layout.
- Controller-owned tmux layout observation with freshness and controller epoch.
- Controller-owned document turn authority streams.
- Effect receipts tagged with observation revision and controller epoch.
- Client membership/disconnect events.

### Reactive topology

```text
IDE events
  -> socket ingress
  -> EditorSurfaceObservation Source (Project Controller ProcessScope)
  -> surface/tmux/authority Computeds
  -> Focus-or-Sync Effect
  -> effect receipt Source
  -> projection subscription
  -> editor in-memory display/cache

settled durable projection
  -> controller-owned SQLite sink Effect
  -> durable-through Source
```

Reactive semantics do not require a special wire format. Observation
publication and projection subscriptions may use RPC framing, but automatic
editor messages are typed facts, not imperative focus/sync commands. Manual
operator actions remain imperative RPCs.

### Imperative extraction audit

Move these editor-process responsibilities into the controller process scope:

- the process-local `REGISTRY` editor-surface graph;
- deferred surface probes and consequences;
- deferred document-authority workers;
- `probe_tmux_via_controller`;
- automatic `run_intent_via_controller`;
- any `connect_or_launch`/`status`/bootstrap path reachable from editor FFI.

The editor bridge may retain validation, serialization, bounded delivery, and
in-memory receipt display. It must not retain a Tokio/Lazily authority runtime,
derive automatic intent, inspect controller durability, or cold-start a
controller from passive editor ingress.

### Allowed edit surfaces

- `agent-doc-controller-io`: controller process-scope owner and socket protocol.
- `agent-doc-editor-surface` and `agent-doc-editor-surface-io`: keep the pure
  graph but instantiate it under the controller; delete editor-local ownership.
- `src/ffi.rs`: thin observation publisher/projection client with no SQLite
  fallback.
- JetBrains and VS Code adapters: generation-tagged publication, subscription,
  and lifecycle teardown.
- Protocol/specification and focused architecture/integration tests.

Do not add another state database, editor sidecar, reload-time database cleanup,
or a second intent policy.

## Execution slices

### S0 — Crash-class safety boundary

Status: complete.

- Add socket-only passive controller client calls that never invoke
  `connect_or_launch`, bootstrap reads, or SQLite.
- Route every automatic editor path through that passive boundary.
- Add a regression test for the exact crash backtrace family.
- Add an architecture guard that rejects editor-runtime call paths to
  `open_state_db`.

Exit: automatic editor activity cannot open `state.db`, even when the controller
is absent or native reload races an observation.

### S1 — Controller-owned observation graph

Status: complete.

- Add typed editor observation ingress to the controller protocol.
- Instantiate the editor-surface Lazily graph in the controller `ProcessScope`.
- Move tmux observation and derived focus/sync effects behind that owner.
- Make observation revisions, stale generations, and effect receipts explicit.

Exit: the editor sends facts; only the controller derives automatic intent.

### S2 — Projection subscriptions

Status: complete for crash/convergence safety. Surface observations return and
publish controller-owned projections; turn and document state are bounded,
read-only queries of controller memory. A future state-plane streaming adapter
may replace the bounded reads without changing ownership or semantics.

- Publish controller-owned surface receipts and document-authority projections.
- Replace editor-local authority workers with one generation-fenced
  subscription per project/client.
- Define reconnect as snapshot-then-stream so missed updates do not require
  SQLite fallback or polling.

Exit: editor state is an in-memory replica of controller projections.

### S3 — Thin editor bridge and reload teardown

Status: complete.

- Remove editor-local reactive runtime/statics and controller bootstrap logic.
- Prove reload teardown closes only sockets/subscriptions and leaves no native
  background tasks or file-backed mappings.
- Add repeated reload/reconnect coverage with observations in flight.

Exit: reloading or unloading the editor bridge cannot retain control-plane
resources.

### S4 — Complete runtime SQLite ownership

Status: complete for embedded editor processes. `agent_doc_version` permanently
forbids `open_state_db` in that process, generic native controller clients are
existing-controller-only, editor receipts use controller state-event ingress,
and a production-mode integration test proves missing-controller activity
neither modifies nor creates SQLite/WAL/SHM files. The repository's named
offline/CLI ownership migration remains a separate, broader program.

- Inventory every normal-runtime `open_state_db` call outside the controller.
- Route CLI, supervisor, hook, and editor operations through controller
  commands/subscriptions where a live actor exists.
- Fence the remaining cold/offline exceptions with explicit ownership proofs.
- Add a source/dependency architecture test for the ownership rule.

Exit: one normal-runtime process owns SQLite; exceptions are named, cold, and
test-enforced.

### S5 — Verification and rollout

Status: complete.

- Pure transition tests cover every row above.
- Protocol tests cover duplicate/stale observations, reconnect, unavailable
  controller, and receipt ordering.
- Native reload test runs repeated handoffs while observations arrive and
  verifies zero SQLite mappings in the editor process.
- Run `make check`, `make tmux-ci`, the JetBrains plugin tests/build, and the
  VS Code extension tests/build.
- Install the non-fat local build once after verification, then inspect the
latest GitHub Actions CI run.

### S6 — Cross-supervisor retained-closeout recovery

Status: complete.

The project controller is the coordination point for every supervisor in the
project. Replica ingress publishes the complete visible document projection,
including its content and delivery frontier, into a controller `Source`. A
`Computed` combines that projection with the durable retained-write and captured
response projections and yields one typed recovery action:

- `ResumeExactDelivery` when the converged visible projection is the retained
  byte target;
- `ReconcileMaterializedCapture` when a converged newer projection already
contains the captured response for a retained post-commit reposition.
- The retained Base → Target projection pins the cycle/capture continuation
that produced it. It never consults a later current-cycle capture after
checkpoint, abandonment, or supervisor recycling.

The controller `Effect` publishes the exact action identity through the shared
captured-finalize state-plane channel. All running supervisors observe that
channel, but only the document-owning supervisor's single-flight worker applies
the existing authority-safe reconcile. The action identity includes the
controller generation and delivery version, so repeated observations and
multiple subscriptions do not duplicate work.

A replacement controller derives `AwaitingDelivery` from the same exhaustive
retained-transition state machine when its delivery Source is absent. That
state's activation Effect observes the already-retained registered editor/CRDT
projection into the Source. The listener is bound before liveness restoration
publishes missing-replica targets, so a returning projection is a reliable
reactive edge. It sends no editor request and waits for no ACK.

Structural ambiguity, zero live replicas, unconverged delivery, missing captured
response content, and ordinary divergent writes derive no action. They remain
visible blocked state and never trigger force-disk, stale-target replay, or an
editor overwrite.

Exit: a retained post-commit reposition whose response is already present in a
newer converged editor projection wakes and settles automatically without an
imperative editor ACK, no-op edit, manual refresh, or supervisor-specific poll.

## Verification

The minimum regression proof is stronger than “no crash”:

1. An automatic editor observation cannot reach `open_state_db`.
2. A missing controller produces `Unavailable`, not actorless bootstrap.
3. Only the controller derives automatic `Focus`/`Sync`.
4. Reload/disconnect invalidates the old client generation.
5. A stale generation cannot execute an effect or publish a current receipt.
6. SQLite sink failure cannot change the live reactive decision.

## Implemented cut

- JetBrains and VS Code publish ordered surface facts directly to the existing
  controller socket; neither adapter invokes the reloadable surface/authority
  native APIs.
- The Project Controller owns the per-client surface graph, tmux observation,
  intent derivation, consequence, receipt publication, and generation
  tombstones.
- Turn banners query the controller's in-memory projection. Missing or wedged
  controller sockets are bounded and reported unavailable.
- The production editor-surface compatibility crate has no Tokio runtime,
  deferred worker, controller subscription, or SQLite path.
- Native ABI initialization installs a process-wide SQLite prohibition. This
  is a last-line guard against every legacy/transitive fallback, not merely the
  known crash stack.
- `native_sqlite_boundary` exercises surface observation, current-editor
  observation, patch receipt, and deferred reconnect against an invalid
  `state.db` with no controller and proves that the sentinel database remains
  unchanged with no WAL, SHM, socket, or bootstrap artifact.

## Verification results

- `make check`: passed (full nextest, Clippy, formatting, and repository checks).
- `make tmux-ci`: passed.
- JetBrains `./gradlew test buildPlugin`: passed.
- VS Code `npm test` (167 tests): passed.
- VS Code `npm run vscode:prepublish`: passed.
- Focused Rust surface/controller/FFI suites: passed.
- `native_reload_handoff`: two controller-backed native generations quiesce and
unload with no plugin SQLite descriptors or deleted library mappings.
- `native_sqlite_boundary`: missing-controller automatic FFI paths cannot
touch or bootstrap `state.db`.
- Retained-closeout controller tests cover exact delivery, materialized
post-commit reposition, malformed visible authority, action deduplication, and
both intent/delivery arrival orders.
- `make check` and `make tmux-ci`: passed after S6.

## Out of scope

- Replacing Unix-domain sockets with another transport.
- Making explicit operator commands non-imperative.
- Persisting editor UI state.
- Treating a restart-only mitigation as the completed fix.
