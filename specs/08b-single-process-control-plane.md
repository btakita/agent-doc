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
generations, dispatch attempts, queue heads, queue controls, queue
backpressure, document cycles, pending backlog mutations, supervisor leases,
admin operations, and projection diagnostics.
- `.agent-doc/proof-ledger/<document-hash>.jsonl` is the append-only operation
proof ledger for mirror-mode cutover rows until those proofs move behind the
store actor. Each row is keyed by `operation_id` plus `content_hash` and covers
queue heads, response captures, patch writes, actor generations, and terminal
proofs. Consumed, deferred, retried, and superseded operations append new rows;
existing rows are never rewritten.
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
- `sessions.json` projection emission is controller-backed once an actor row
  exists: the projection worker derives the binding from SQLite, preserves
  existing compatibility metadata when available, can recreate a missing
  registry entry from the actor row, and removes stale same-pane legacy entries
  that conflict with a live controller actor.
- Session actor closeout persists the selected queue head, response/cycle
terminal state, and tracked-work mutations as one controller transaction
after strict closeout checks pass. Failed closeout must leave those controller
rows unadvanced.
- Cycle-state authority is split into an accepted transition and a durable
snapshot. `cycle_state_machine::CyclePhaseMachine` is the pure transition
authority for `CycleEvent` phase changes (`StartPreflight`,
`ResponseCaptured`, `WriteApplied`, `Committed`, `Abandoned`, and recovery
rewinds). The compatibility `cycle_state` sidecar path applies that transition
table before it writes `.agent-doc/state/cycles/<hash>.json`; later controller
cutover must submit the same events through the session actor and emit the
sidecar from the serialized owner job. The sidecar remains the crash-recovery
journal and startup replay source; it is not deleted by controller cutover.

## Workflow state kernel

Cross-cutting route, queue, closeout, and editor-write policy is represented as
one pure workflow-state kernel. Runtime paths gather evidence once, pass it to
the kernel, and receive a typed transition:

- evidence kind: stale supervisor, queue drainability, captured response, or
live buffer;
- decision enum: the policy outcome, such as recycle the stale supervisor,
dispatch/defer a queue head, append/replay/retire a captured response, or
apply/request-save/defer an editor-visible write;
- allowed mutation: the only mutation the caller may perform for that decision,
such as `ReexecSupervisor`, `InjectQueueDrainTrigger`,
`ReplayCapturedResponse`, `RetireCapture`, or `RequestEditorSave`;
- required proof: the proof the caller must have before performing the mutation,
such as turn-boundary, queue-head identity, response-body hash, capture record,
supersession proof, live-buffer hash/provenance, or editor-idle proof.

The first implementation is mirror-mode in `agent_doc_workflow`: existing
route, idle-queue, closeout, and write paths still gather evidence and perform
I/O, but the transition rows are now typed and unit-covered. Later controller
cutover work must replace ad hoc guard branching with calls to this kernel
before adding more route-edge guards.

## Workflow invariant catalog

`agent_doc_workflow::invariants` owns the machine-readable invariant catalog
used by doctor/autofix work. Catalog entries have stable ids, severity,
declared fact sources, an `ok_predicate`, disproof markers, safe remediation,
operator-gated remediation, and SimWorld/regression coverage. Orchestration
doctor/autofix commands consume the focused catalog directly. The initial
catalog covers:

- `queue_continuation`
- `stale_supervisor`
- `closeout_commit`
- `editor_convergence`
- `generation_redirect`
- `parent_gitlink`

The catalog is data-only and serializable as
`workflow-invariant-catalog-v1`; runtime diagnosis must evaluate these fields
instead of scraping prose from session docs or specs.

`agent-doc doctor <FILE>` is the diagnostic surface for that catalog.
It gathers optional preflight/session-check JSON, live `session-check`
inspection, cycle state, recent `ops.log` markers, best-effort controller actor
freshness, git/snapshot state, parent gitlink state, and editor sidecar presence.
Each invariant reports one typed outcome: `ok`, `recoverable`, `operator`, or
`blocked`, plus exact repair commands or operator actions. Missing required fact
sources are `blocked` with the command needed to gather the evidence; detected
safe repairs are `recoverable`.

`agent-doc autofix <FILE>` consumes the doctor report and catalog remediation
classes to build an invariant-keyed repair plan. The command records
`workflow_autofix:<invariant>:<hash>` proof rows in the append-only proof ledger
so repeated symptoms de-duplicate by invariant id and fingerprint before they
create more queue/backlog work. `--apply` executes only whitelisted safe repairs
whose catalog proof is present; destructive, shell-compound, git-commit,
generation-reroute, queue-drain, and editor-conflict actions stay gated with an
operator/manual command instead of being auto-run.

## Document write and watch authority

The store actor owns `.agent-doc/state.db` as the single durable-state writer.
This section extends that authority to the **session `.md` document file on disk**
and its **filesystem watch**, which today are split across the binary write
paths, the supervisor idle/file-watch path, and the editor plugin WatchService
with only a per-operation advisory `flock` between them. That split is the
structural root of the "File Cache Conflict" / IPC-drift / supervisor self-race
family (the binary disk fallback and the supervisor both write; the plugin
WatchService and the binary both watch and both write; drift is inferred from an
mtime heuristic rather than positively attributed). Consolidating it removes the
race class instead of papering over each symptom.

- **Single disk writer.** The session actor for a document generation is the only
  writer of that `.md` on disk. All `write`/`stream`/`finalize`/repair disk
  writes and all supervisor idle-queue/file-watch writes are submitted to that
  session actor and serialized through **one in-process ordered write queue**.
  Submissions carry their `OpActor` provenance and the turn/operation scope. The
  cross-process advisory `flock` remains a backstop against a foreign process,
  not the primary serializer; within the controller process, ordering is the
  write queue's responsibility. This eliminates the supervisor self-race where a
  route-owned supervisor write and an agent finalize write interleave on the same
  pane ("could not drain the active closeout" / exit 75).
- **Single filesystem watcher.** The controller owns one filesystem watcher per
  live document and feeds its events to the session actor. Editor plugins
  (JetBrains WatchService, VS Code file watcher) are demoted to **read-only
  buffer-state reporting**: they report buffer content, version, and dirty state
  through the existing typing/digest FFI and never autonomously reconcile or
  write the document. A single watch authority removes the two-watchers/two-
  writers conflict the editor surfaces as a memory-vs-disk "File Cache Conflict".
- **Write provenance.** Every controller/session-actor disk write is stamped with
  a write-provenance generation id plus its `OpActor`, durably recorded alongside
  the write (provenance sidecar or extended live-buffer digest). This is enforced
  uniformly: both agent-doc document-write paths — the IPC/finalize/queue
  `write.rs::atomic_write` and the direct-run `run.rs::atomic_write` — stamp through
  the one shared `record_document_write_provenance` recorder (`#ipc-drift-writeprovenance`),
  so no document-write path can land a foreign-looking change unattributed. The visible-write
  reconcile guard (`live_buffer_diverges_from_content` / `guard_visible_write_reconcile`)
  attributes a foreign disk change to the controller/supervisor by
  **reading that provenance**, not by inferring foreign-vs-unsaved-edit from the
  `LIVE_BUFFER_STALE_SKEW_MS` mtime skew. The mtime heuristic may remain only as a
  shadow-mode fallback during migration. Provenance builds on the existing
  `OpActor`/`OpSource`/`CausalClock` op-log substrate.
- **Thin editor apply/ack shim.** The FFI socket listener and the editor plugin
  are reduced to an EDT apply+ack shim: receive a patch, apply it via the editor
  Document API, send an early `accepted`/`pending` ack on receipt (before the
  idle-wait apply window), then a terminal ack. The shim performs no independent
  reconcile and no disk write; the controller's session actor remains the sole
  disk-write authority and a genuinely degraded session routes through the
  file-IPC patch queue (plugin still applies via Document API) rather than a raw
  disk write that would manufacture a File Cache Conflict.

Migration follows the same gate ladder as the rest of this contract (shadow →
dual-write → read-switch → authority → removal); each gate carries a rollback
flag and must not lose committed controller rows. Behavior changes here must
never be landed in the same cycle that persists a live session response through
the write path being changed.

The document-write authority cutover is **complete**. It is applied at the
single `write::atomic_write` chokepoint for editor-visible documents
(`.agent-doc/` sidecar and snapshot writes are never routed). Both same-process
document writers reach that one chokepoint: the IPC/finalize/queue
`write.rs::atomic_write` directly, and the direct-run `run.rs::atomic_write` by
delegating to `write::atomic_write_pub`. Route-owned queued rerun writes are the
one separate editor-visible mutation family; they must call the shared
`converge_document_or_disk` gate before saving the matching snapshot, so a live
IDE listener receives component/frontmatter IPC instead of a direct disk write.
There is no parallel direct-disk writer that bypasses the queue.

Every editor-visible document write now routes through the session actor's
single ordered write queue (`write_queue::serialized_atomic_write`)
unconditionally; a write already executing on the session-actor owner thread
(`write_authority::within_owner_scope`) takes the raw path to avoid re-entering
its own blocking mailbox. The cross-process advisory `flock` remains only a
foreign-process backstop. This removes the in-process supervisor/finalize
interleave at the root.

This shipped through the `AGENT_DOC_WRITE_AUTHORITY` rollback flag ladder
(`off → shadow → dual-write → authority → removed`). At the removal rung the
flag, the `WriteAuthorityGate` enum, and the bare-`atomic_write` `off` bypass
were deleted, so routing is now unconditional — there is no rollback flag.
The companion supervisor-host cutover (retiring the out-of-process `agent-doc
start` host in favor of the in-process adapter `supervisor::in_process`, and the
WatchService read-only demotion) is likewise complete; see the filesystem-watch
authority note below.

The editor-plugin filesystem-watch demotion (`#dsqa`/`#pcp7`) is **complete**.
It shipped through the `AGENT_DOC_PLUGIN_WATCH` rollback flag (`active →
read-only`); at the removal rung the flag and the `active` (plugin-applies) path
were removed, so the plugin's autonomous NIO `WatchService` file-apply path is
**unconditionally** read-only. The plugin no longer applies file-IPC patches it
observes under `.agent-doc/patches/`; the single controller-owned watcher plus
the socket IPC command channel (the controller's writer arm into the editor) are
the sole writer to the live buffer, removing the second-watcher race that
mutated the live buffer between an agent finalize's preflight and commit. The
plugin reads this end state through the `agent_doc_plugin_watch_readonly` FFI
export (`watch_authority::plugin_watch_is_readonly`, always true), which emits a
structured `plugin_watch_readonly` `ops.log` marker so the demotion is
log-verifiable. The plugin's socket IPC apply path stays active.

Routing a write through the queue re-enters `atomic_write` on the session-actor
owner thread; the thread-local owner-scope re-entrancy guard
(`write_authority::owner_scope_guard`, installed by
`write_queue::run_serialized`) keeps that inner write on the raw path so a routed
write cannot deadlock the document's blocking mailbox. Rolling a gate back is
unsetting the env (or setting `off`), which returns the previous write authority
without losing any committed state. Because the gate is keyed off an env var and
default-`off`, advancing it is an out-of-cycle operator action — never landed by
an agent response cycle persisting through this same `write.rs` path.

Deterministic SimWorld coverage for this authority must model at least:

- a supervisor idle/file-watch write and an agent finalize write submitted
  concurrently for one document, serialized deterministically through the single
  write queue, with finalize never observing a half-applied supervisor write;
- a foreign-supervisor disk write positively attributed by write provenance, and
  a genuine unsaved editor edit (editor ahead of disk) that is NOT misclassified
  as a foreign write and does not fail closed (`live_prompt_drift_after_preflight`);
- a single watcher event stream driving typing/idle classification with no
  duplicate-watcher reconcile race;
- an early-ack decoupling sender liveness from apply latency, and a degraded
  session that routes through file-IPC instead of a raw disk write.

Live IntelliJ/VS Code coverage remains a smoke layer for the editor buffer-report
and apply/ack boundary; the serialization and provenance matrix belongs in
deterministic tests.

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
proof. The current controller authorization API persists each dispatch decision
as a `dispatch_attempts` receipt before returning: responses include
`receipt_id`, `status`, `stage`, `accepted_stage`/`failed_stage`, `proof_scope`,
and `dispatch_start_proven`. Rejected and blocked decisions must also commit
their receipt before the caller sees the rejection.
Queue-blocked dispatches also attach non-sensitive proof fields to the receipt
diagnostic payload and route error: active queue head byte count/hash and, when
the caller names a harness, the harness trigger byte count/hash. The controller
must not persist raw prompt text for this proof path.

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

The controller exposes `admin_operation` as the durable receipt target for
admin-class commands such as `start`, `sync`, `session status`, `repair`,
`preflight`, and `write`/`finalize` as they migrate off sidecar-first paths. The
request records an operation kind, optional document id, typed status, and
diagnostic payload in `admin_operations`, then returns the receipt id to the
caller.

## SupervisorHandoff State Machine

A true red/green supervisor replacement must be owned by the project controller
as a `SupervisorHandoff` state machine. The existing route-owned supervisor
`execve` hot reload remains the default restart path because it preserves the
live child and pty without cross-process fd transfer. The handoff state machine
only applies when PCP starts a separate replacement supervisor process.

States:

- `Idle`: no handoff is active.
- `LaunchingStandby`: PCP launches a fresh supervisor on a private socket.
- `ProbingStandby`: PCP verifies standby freshness, capabilities, and health.
- `AwaitTurnBoundary`: standby is healthy but fenced; old supervisor is still
  authoritative until `prompt_visible && !turn_active`.
- `PromotingLease`: PCP compare-and-swap promotes the supervisor lease and
  generation at the turn boundary.
- `TransferringOwnership`: promotion committed; the new supervisor may adopt
  child/pty ownership.
- `StoppingOld`: ownership transferred; PCP stops the old supervisor generation.
- `Complete`: the new supervisor owns the lease and child.
- `RollingBack`: pre-promotion failure or abort; terminate standby and keep the
  old supervisor active.
- `BlockedPostPromotion`: promotion committed but ownership transfer or stop
  failed; require repair instead of automatic rollback.

Transitions:

- `Idle` + `RequestAccepted` -> `LaunchingStandby`, action `LaunchStandby`.
- `LaunchingStandby` + `StandbyStarted` -> `ProbingStandby`, action
  `ProbeStandby`.
- `ProbingStandby` + `StandbyHealthy` -> `AwaitTurnBoundary`, action
  `WaitTurnBoundary`.
- `AwaitTurnBoundary` + `TurnBoundaryReached` -> `PromotingLease`, action
  `PromoteLease`.
- `PromotingLease` + `PromotionCommitted` -> `TransferringOwnership`, action
  `TransferChildOwnership`.
- `TransferringOwnership` + `OwnershipTransferred` -> `StoppingOld`, action
  `StopOldSupervisor`.
- `StoppingOld` + `OldSupervisorStopped` -> `Complete`, action
  `CompleteHandoff`.
- Any pre-promotion standby failure, failed promotion, or abort -> `RollingBack`,
  action `RollbackStandby`.
- `RollingBack` + `RollbackComplete` -> `Idle`.
- Any post-promotion standby failure or abort -> `BlockedPostPromotion`, action
  `EscalateRepair`.

Invariants:

- The old supervisor remains the only authoritative child/pty/session-socket
  owner through `PromotingLease`.
- The supervised pane identity is stable across replacement. The standby/new
  supervisor must inherit the active actor record's `pane_id`; it must not
  rediscover the controller caller's current pane or bind a fresh pane unless an
  explicit attach/new-pane actor transition is part of the handoff.
- A standby supervisor must not read/write the child pty, dispatch queue work,
  heartbeat as the active lease, or write the session document before
  `PromotionCommitted`.
- PCP promotion is compare-and-swap guarded by document id, observed old
  generation, standby generation, and private standby socket identity.
- Automatic rollback is allowed only before lease promotion. After promotion,
  ambiguous ownership is repair-only so two supervisors cannot race the same
  child or session state.

Controller queue state is stored in SQLite rather than loose marker files:

- `queue_heads`: document id, queue name, actor generation when known, head id,
  prompt text, state, priority, selected timestamp, and updated timestamp;
- `queue_controls`: document/project scope, paused/resumed/draining state,
  operator reason, linked admin operation receipt id, and updated timestamp;
- `queue_backpressure`: document id, actor generation when known, rejected or
  blocked command kind, capacity class (`queue_full`, `queue_paused`,
  `actor_busy_draining`, etc.), reason, linked dispatch receipt id, and
  timestamp.

Actor inspection responses include the current queue head/control state and
recent typed queue backpressure receipts, alongside dispatch/admin receipts and
projection diagnostics.

Editor integrations use the same controller contract through the C ABI
`agent_doc_admin_inspect_json`, `agent_doc_admin_queue_control_json`,
`agent_doc_admin_reap_json`, `agent_doc_admin_handoff_json`, and
`agent_doc_admin_repair_projection_json` wrappers. Each wrapper returns an
`FfiJsonResult` containing the same JSON inspection or typed admin receipt
envelope as the CLI `--json` form; null or empty optional string arguments mean
"not supplied", and optional generation guards use `-1` for "none". Editor
plugins must not infer ownership or mutate sidecars around these wrappers.

`agent-doc admin dashboard` surfaces controller inspection diagnostics in
addition to fleet list/detect rows. Text and JSON dashboard output include the
effective queue control state, the latest typed queue pressure class, and
projection lag per actor. Queue pressure and projection lag flag the row so
operators and editor status surfaces can distinguish backpressure/projection
problems from generic busy-pane failures.

The first controller-backed mutating CLI surface is:

- `agent-doc admin inspect <document|--session <id>|--pane <pane>> [--json]`;
- `agent-doc admin queue pause|resume [document|--project-root <root>]`;
- `agent-doc admin queue drain <document> [--until-id <id>]`;
- `agent-doc admin reap <document|--session <id>|--pane <pane>>
  --observed-generation <n> --reason <text>`;
- `agent-doc admin handoff <document> --to-pane <pane>
  --observed-generation <n> --reason <text>`;
- `agent-doc admin repair-projection [document] --projection all|actors|sessions|layout`.

Document-scoped mutating operations that target an existing actor require the
caller's observed generation. Missing or stale generations return an
`admin_operations` receipt with `status=rejected`, `failed_stage`, and the
current generation; they do not mutate queue controls, actor bindings, or
projections.

`agent-doc controller status` must expose the controller-owned runtime shape in
its `control_plane` JSON field. That field identifies the project-scoped
single-process model, the controller IPC external boundary, `.agent-doc/state.db`
state authority, compatibility-only projection authority, and role snapshots for
the dispatch actor, store actor, per-document session actors, supervisor
adapters, and projection workers. Store role snapshots include per-category
SQLite counts for actor documents, lifecycle transitions, supervisor leases,
dispatch receipts, queue heads, queue controls, queue backpressure, document
cycles, tracked-work mutations, projection diagnostics, admin operations,
crash-recovery markers, and layout state.
`agent-doc controller status` and `agent-doc admin inspect --json` must also
include a `freshness` object that compares running controller/supervisor binary
inodes against the installed agent-doc binary inode when the platform exposes
that proof. `admin inspect` includes the route-owned supervisor process when the
target actor has a supervisor lease. Non-JSON inspect output summarizes the same
state as `freshness=controller:<state>,supervisor:<state>`.

Every controller RPC request stamps the caller's complete binary identity as
skew-safe optional JSON metadata. A `stale_controller_replacement` shutdown is
accepted only when that identity is directionally newer than the controller's
bootstrap identity: a higher dotted release version, or a later executable
mtime for a same-version reinstall. An older caller adopts a newer controller.
The controller must not judge replacement freshness only from its own
process-local executable, because that identity necessarily still matches the
old bootstrap and would otherwise refuse every legitimate installed successor.

Supervisor auto-install (`AGENT_DOC_SUPERVISOR_AUTO_INSTALL` /
`agent_doc_supervisor_auto_install`) is a dogfood-only lifecycle policy. The
idle supervisor may run `make install` only when the served document is an
agent-doc dogfood session document: a document inside the agent-doc source checkout, under
`tasks/agent-doc/`, or one of the legacy agent-doc task documents. A sibling
project session in the same superproject, such as
`tasks/professional/sampleportal.md` or `tasks/software/lazily-rs.md`,
must not resolve `src/agent-doc` as its auto-install crate root even when the
env/frontmatter/project auto-install knob is truthy.

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
- On controller generation restart, startup runs a bounded recovery sweep before
  accepting new requests: it reloads actor memory from SQLite, reconciles fresh
  supervisor leases as reattached or stale, records retryable/blocked dispatch
  receipt recovery markers based on side-effect proof scope, emits
  `session-actors.json`, `sessions.json`, and layout projections from committed
  rows, and records open closeout cycles as preserved without advancing queue or
  backlog rows.

## Backpressure

- Dispatch queues are bounded per document and per project.
- Prompt-bearing work for a busy document is queued ahead of lower-priority
  automatic backlog work when it is an explicit user/editor dispatch.
- Background projection/log/metric workers use coalescing queues so slow I/O
  cannot make route/start/session-clear wait on JSON or ops-log writes.
- When queues are full, the controller returns a typed backpressure rejection
  with the actor id, queue depth, oldest receipt age, and the admin command that
  can pause, inspect, or drain the work.
- When a durable queue control pauses a document or project, dispatch must return
  `Blocked` with `failed_stage=queue_paused`, persist the dispatch receipt, and
  add a `queue_backpressure` row before any external input is sent. When a
  document is draining and the actor is not ready, dispatch must return
  `failed_stage=actor_busy_draining` through the same receipt/backpressure path.
- A redundant in-flight re-dispatch (an identical dispatch for the same cycle is
  already accepted and unconsumed) is coalesced: the controller records the
  `coalesced_in_flight` receipt and suppresses the re-send so the routed trigger is
  never piled into the busy pane. Because the requested work is already in flight,
  this is a benign dedup, not a failure — route callers must recognise the coalesce
  (it carries the stable `failed_stage=coalesced_in_flight` marker across the IPC
  boundary), skip the re-send, and report deduped-success on the already-running
  dispatch pane rather than surfacing an exit-1 to the operator (`#qflood2`). An
  explicit operator dispatch is never coalesced.

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
- Codex, Claude Code, and OpenCode dispatch proof differences;
- concurrent supervisor and finalize disk writes serialized through the single
  document write queue, with provenance-attributed foreign writes and a genuine
  unsaved editor edit that is not misclassified (see Document write and watch
  authority).

CLI integration coverage must prove the same controller API is used by `route`,
`start`, `sync`, `session status`, `session clear`, `session restart`, `repair`,
`preflight`, and `write/finalize` closeout paths. Live tmux coverage remains a
small smoke layer for the external pane boundary; the state-machine and
concurrency matrix belongs in deterministic tests.
