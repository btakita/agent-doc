> Supplement to [08-session-routing.md](08-session-routing.md) and
> [08a-session-actor-contract.md](08a-session-actor-contract.md)

# Single Process Control Plane Contract

This file defines the target control-plane architecture for consolidating the
project controller actor, document session actors, and supervisor actors into one
project-scoped process. It preserves the existing one-owner-per-document
semantics while moving mutable coordination state out of ad hoc IPC, JSON
projections, and tmux transcript inference.

## Scope

- One controller process owns one project root. The process may host many
  document actors and many supervisor adapters.
- Agent harnesses remain child processes. Claude Code, Codex, OpenCode, tmux,
  and editor plugins are still external systems.
- IPC remains the boundary between external callers and the controller process:
  CLI commands, editor plugins, and managed panes send requests through the
  controller API instead of mutating session sidecars directly.
- In-process consolidation does not make every operation synchronous. The hot
  path validates commands and records durable intent quickly; slow work runs in
  document actors, supervisor adapters, or background workers.

## In-process actors

- The dispatch actor is the only admission point for mutating commands. It
  validates document path, session id, pane id, generation, command kind, and
  queue policy, then returns a receipt or a typed rejection without waiting on
  projection files or long harness operations.
- The store actor is the only writer for authoritative durable state. It owns a
  single SQLite connection in WAL mode and serializes transactions for actor
  state, lifecycle transitions, dispatch attempts, queue state, projection
  status, admin operations, and crash-recovery markers.
- A session actor exists for each live document generation. It owns closeout
  cycle state, active queue head selection, response/pending mutation ordering,
  and document-level command serialization.
- A supervisor adapter exists for each managed harness child. It owns child
  process state, PTY/stdin delivery, readiness evidence, restart/clear control,
  and heartbeat reporting for that child.
- Projection workers emit compatibility files and diagnostics from committed
  controller state. They never establish ownership and never block command
  admission.

## State authority

- `.agent-doc/state.db` is the authoritative store for actor records,
  generations, dispatch attempts, queue heads, document cycles, supervisor
  leases, admin operations, and projection diagnostics.
- `session-actors.json`, `sessions.json`, layout JSON, session logs, and
  `ops.log` are compatibility or diagnostic projections. Normal route, start,
  sync, clear, restart, and queue-dispatch paths must not treat them as write
  authorities once the controller record exists.
- The in-memory actor map is a write-through cache of SQLite state. A successful
  mutation updates memory and commits one SQLite transaction before reporting an
  accepted state-changing result.
- The first in-memory implementation may use the controller-local standard
  `BTreeMap` snapshot. `lazily-rs` remains an implementation candidate for
  indexed/lazy map maintenance, but it must preserve the same synchronous
  write-through contract: accepted mutations update the live memory authority
  only after the SQLite transaction commits, and failed commits must leave the
  memory snapshot unchanged.
- JSON projection failures are recorded as projection diagnostics with source
  generation, intended projection hash, error, and retry status. They must not
  roll back or weaken the authoritative actor state.

## Lifecycle invariants

- A document has at most one non-closed authoritative actor generation at a
  time.
- Generation changes are compare-and-swap guarded by document id and prior
  generation. A stale caller cannot replace a newer generation.
- Same-generation lifecycle updates are compare-and-swap guarded by document id,
  session id, pane id, and generation. A stale supervisor adapter or CLI request
  cannot mark a different owner ready, busy, blocked, or closed.
- Every accepted command receives a durable dispatch receipt before external
  side effects such as tmux input, supervisor stdin, response write-back, or
  projection emission.
- Queue-head selection and backlog item completion are committed in the same
  closeout transaction as response/cycle terminal state whenever a `do #id`
  item is completed. A response must not claim tracked work was captured or
  completed without the matching backlog mutation in the same committed cycle.
- Actor state transitions are append-only facts in `actor_transitions`; current
  rows are materialized views over those facts for fast reads.

## Dispatch API

The dispatch actor exposes a small real-time API:

- `SubmitCommand`: validate and enqueue prompt-bearing work, operator commands,
  queue continuations, closeout repair, or projection repair.
- `QueryActor`: return the authoritative actor record, generation, state,
  supervisor lease summary, queue head, and recent failed guard.
- `QueryDispatch`: return receipt status, stage, accepted/rejected reason,
  target actor, and proof requirements.
- `QueueControl`: pause, resume, reorder, promote, gate, or drain document queue
  work through the same queue transaction path used by closeout.
- `AdminControl`: restart supervisor adapter, close stale actor, force handoff,
  clear projection errors, or run scoped doctor/repair operations.

Dispatch results are typed as `Rejected`, `Accepted`, `Queued`, `Running`,
`Completed`, or `Blocked`. A pane-input acceptance is not a completed dispatch;
proof scope must still distinguish accepted-only delivery from dispatch-start
proof.

## Admin API

The admin API is controller-backed and sufficient to manage the actor pool
without direct sidecar edits:

- list actors by project, document, state, harness, pane, generation, and age;
- inspect one actor with current lifecycle state, supervisor lease, queue head,
  dispatch receipts, projection lag, and last transition;
- list and inspect supervisor adapters, child PIDs, sockets, readiness evidence,
  proof gates, and recent restart/clear attempts;
- pause/resume queue draining for a document or project;
- close or reap stale actors with a typed reason and compare-and-swap guard;
- force a manual handoff only when the caller provides the observed generation
  and the command records supersession provenance;
- trigger projection repair or doctor checks without changing actor ownership;
- stream controller health, projection lag, and dispatch metrics for editor
status surfaces.

`agent-doc controller status` must expose the controller-owned runtime shape in
its `control_plane` JSON field. That field identifies the project-scoped
single-process model, the controller IPC external boundary, `.agent-doc/state.db`
state authority, compatibility-only projection authority, and role snapshots for
the dispatch actor, store actor, per-document session actors, supervisor
adapters, and projection workers. Store role snapshots include per-category
SQLite counts for actor documents, lifecycle transitions, supervisor leases,
dispatch receipts, queue heads, document cycles, projection diagnostics, admin
operations, crash-recovery markers, and layout state.

Read-only admin calls may be served from the in-memory snapshot when the result
includes the snapshot generation/version. Mutating admin calls must enter through
the dispatch actor and produce receipts.

## Crash and restart

- Startup rebuilds the in-memory actor map from SQLite, not from JSON
  projections.
- Open dispatch receipts are reconciled before accepting new work. Receipts with
  no external side-effect proof may be retried idempotently; receipts with
  ambiguous side-effect proof become `Blocked` with a repair action instead of
  being replayed blindly.
- Supervisor leases carry heartbeat timestamps and child identity. A live child
  with a matching fresh lease may be reattached; a stale lease is closed or
  marked blocked according to the same startup/ready guards used by route.
- Projection workers resume from the newest committed generation and coalesce
  redundant writes. Projection lag is diagnostic; it is not actor authority.
- If the controller crashes after committing state but before emitting JSON or
  logs, restart must emit the missing projections from committed SQLite state.
- If the controller crashes before committing a response closeout, the durable
  response capture and cycle state decide repair, and the queue/backlog state
  must remain unadvanced.

## Backpressure

- Dispatch queues are bounded per document and per project.
- Prompt-bearing work for a busy document is queued ahead of lower-priority
  automatic backlog work when it is an explicit user/editor dispatch.
- Background projection/log/metric workers use coalescing queues so slow I/O
  cannot make route/start/session-clear wait on JSON or ops-log writes.
- When queues are full, the controller returns a typed backpressure rejection
  with the actor id, queue depth, oldest receipt age, and the admin command that
  can pause, inspect, or drain the work.

## Migration gates

- Shadow mode writes the SQLite controller state beside existing JSON behavior
  and reports drift.
- Dual-write mode treats SQLite writes as required while still emitting JSON
  projections for compatibility.
- Read-switch mode makes route/start/sync/operator commands read SQLite first
  and JSON only as a projection fallback when no controller record exists.
- Authority mode removes JSON fallback from normal ownership decisions and
  leaves JSON for compatibility and diagnostics only.
- Removal mode deletes obsolete direct JSON writers after SimWorld, CLI, and
  live tmux coverage prove the controller path for all supported harnesses.

Every gate must have a rollback flag that returns the previous read authority
without losing committed controller rows.

## Verification obligations

Deterministic SimWorld coverage must model at least:

- concurrent route/start/sync/clear commands racing on the same document;
- stale session id, pane id, and generation rejection;
- busy actor queueing and queue-head promotion;
- response closeout plus backlog completion in one transaction;
- controller crash before and after SQLite commit;
- projection lag and projection write failure;
- supervisor heartbeat loss and reattach;
- admin handoff, stale actor reap, queue pause/resume, and projection repair;
- Codex, Claude Code, and OpenCode dispatch proof differences.

CLI integration coverage must prove the same controller API is used by `route`,
`start`, `sync`, `session status`, `session clear`, `session restart`, `repair`,
`preflight`, and `write/finalize` closeout paths. Live tmux coverage remains a
small smoke layer for the external pane boundary; the state-machine and
concurrency matrix belongs in deterministic tests.
