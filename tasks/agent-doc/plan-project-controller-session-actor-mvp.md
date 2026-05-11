# Plan — Project Controller / Session Actor MVP

**Pending:** `#coordspike`
**Status:** Draft

## Problem

The `#tmuxcoord` investigation showed that the recent three-pane and
non-responsive `Run Agent Doc` failures are not primarily tmux-router layout
bugs. They are ownership bugs. Different surfaces can still disagree about the
document owner, supervisor state, prompt readiness, and whether a stale pane is
allowed to receive the next trigger.

The existing single-owner session actor work is the correct direction, but the
current implementation still spreads live coordination across CLI commands,
supervisor state, tmux registry projections, session logs, and recovery
sidecars. A durable actor record helps, but it does not by itself ensure one
live process serializes project-wide session decisions.

Use this hierarchy for the MVP:

```text
Project Controller -> Session Actor -> Harness Supervisor -> Harness Child
```

- `Project Controller`: one project/workspace-level service. It owns singleton
  socket identity, launch locking, session actor lookup/creation, and global
  recovery policy.
- `Session Actor`: one authoritative owner for one document session. It owns
  document id, session id, generation, pane/window binding, harness id, state,
  and valid transitions.
- `Harness Supervisor`: one runtime wrapper around a live Codex/Claude child.
  It owns pty/process lifecycle, readiness observation, restart/clear execution,
  and reports observations to its owning session actor.
- `Harness Child`: the actual Codex/Claude process.

## Goal

Build the smallest controller/actor slice that removes normal-path ownership
election from route/sync/supervisor code while preserving the current user
workflow:

- editor `Run Agent Doc` dispatches to the authoritative generation
- `agent-doc start`/`route`/`sync` cannot create duplicate hidden owners
- stale `starting` or dead supervisor state is resolved by an explicit actor
  transition, not by tmux heuristic recovery
- `sessions.json` and tmux-router state remain projections and layout inputs,
  not independent authorities

## Non-Goals

- Do not rewrite every sidecar into SQLite in the first slice.
- Do not introduce a distributed supervisor-to-supervisor coordination protocol.
- Do not make tmux-router responsible for session ownership.
- Do not require a permanently installed OS service before local use works.
- Do not let the controller silently kill or replace panes on the normal path.

## Architecture Decision

Use one local Project Controller as the live authority, with SQLite as its
durable journal/state store.

This is preferable to having each Harness Supervisor coordinate directly through
the same SQLite database. SQLite can make writes atomic, but it does not make
multiple supervisors agree on leases, stale owner cleanup, prompt-state
interpretation, conflict resolution, or recovery ordering. The MVP should keep
those decisions in one process and use SQLite for persistence and restart
recovery.

## MVP Shape

### 1. Singleton controller identity

- Add one project-local IPC endpoint under `.agent-doc/`, for example
  `.agent-doc/controller.sock` on Unix and a named-pipe or loopback fallback
  where needed.
- Add a short launch lock such as `.agent-doc/locks/controller-launch.lock`.
- Client startup flow:
  1. connect to the controller socket
  2. if unavailable, acquire the launch lock
  3. recheck the socket
  4. if still unavailable, launch the controller detached
  5. wait for a bounded readiness signal
  6. connect as a normal client
- Record launch mode:
  - `managed`: explicit daemon/user/service start; stays up until stopped
  - `lazy`: launched on first demand; may self-exit after idle grace

### 2. Session actor map

- The controller owns a map keyed by canonical document path.
- Each map entry is a serialized Session Actor mailbox/state machine.
- A Session Actor is created only through controller commands such as `start`,
  `claim`, `attach`, or `recover`.
- Actor transitions remain monotonic:
  - new owner generation for start/attach/recover handoff
  - same-generation lifecycle updates for ready/busy/waiting-input/blocked/closed
  - stale writer or mismatched pane/session id fails closed

### 3. SQLite journal and recovery store

Start with a minimal `.agent-doc/state.db` slice:

- `documents`
  - canonical path
  - session id
  - current generation
  - pane/window id
  - harness id
  - actor state
  - launch mode / controller epoch reference
  - last transition id
- `actor_transitions`
  - document id
  - prior/new generation
  - caller
  - reason
  - old/new pane and window
  - timestamp
- `supervisor_leases`
  - document id
  - generation
  - supervisor pid/socket
  - last heartbeat
  - runtime state
- `dispatch_attempts`
  - document id
  - generation
  - command kind
  - accepted/failed stage
  - diagnostic payload
  - timestamp

Keep current JSON/text outputs as projections during migration:

- `.agent-doc/session-actors.json`
- `.agent-doc/sessions.json`
- `.agent-doc/logs/*.log`

The controller writes SQLite first, then emits projections from committed state.
If projection emission fails, the actor state remains authoritative and
diagnostics must report projection drift.

### 4. Harness Supervisor client protocol

Define a narrow request/response protocol over local IPC. JSON lines are enough
for the MVP; a binary codec can wait.

Controller-facing commands:

- `StartSession { file, harness, cwd, env, launch_options }`
- `AttachPane { file, pane_id, window_id, harness }`
- `Dispatch { file, generation, kind, payload }`
- `MarkLifecycle { file, generation, state, reason }`
- `Restart { file, generation, fresh }`
- `Clear { file, generation }`
- `Close { file, generation, reason }`
- `Status { file }`
- `History { file }`

Supervisor reporting rules:

- The supervisor never mutates ownership records directly.
- Startup reports `starting -> ready` or `starting -> blocked/closed`.
- Routed injection reports `ready -> busy`, then later `busy -> ready`,
  `waiting_input`, `blocked`, or `closed`.
- Clean harness exit reports `waiting_input` when the supervisor can restart,
  and `closed` when it cannot.
- Heartbeats keep the controller from treating a healthy supervisor as stale.

### 5. Route/start/sync migration boundary

- `agent-doc start` asks the controller to create or reject an owner generation.
- `agent-doc route` asks the controller for authoritative dispatch and waits for
  an accepted/failed stage.
- `agent-doc sync` asks the controller for authoritative pane bindings and then
  uses tmux-router only to reconcile layout around those bindings.
- `agent-doc session status/history/attach/restart/clear/doctor` become thin
  controller clients.
- Legacy registry/session-log/process-tree evidence remains diagnostic input
  for `doctor` and explicit repair only.

Performance guardrails:

- `agent-doc sync` records `sync_latency` ops for window resolution, prune,
  ownership proof, tmux-router reconciliation, and safe-passive total runtime.
  A no-op safe-passive sync should stay within the 1s total budget once Phase D
  moves ownership lookup onto controller bindings.
- `agent-doc route` records `route_latency` ops for direct pane submit,
  supervisor IPC submit, dispatch-start proof wait, and routed cycle ack wait.
  A 10s Codex dispatch-proof wait must surface as
  `phase=dispatch_start_proof ... outcome=unproven_but_accepted`, so editor
  users can distinguish "pane accepted the trigger but did not prove
  consumption" from tmux-router or controller latency.
- Over-budget latency records are diagnostics, not hidden recovery triggers.
  They should fail rollout verification if the slow phase remains on the normal
  path after the corresponding controller phase lands.

### 6. Lazy self-exit

Lazy controllers may exit only when all of these are true:

- no connected supervisor clients
- no unexpired supervisor leases
- no dispatch or closeout operation is in progress
- idle grace period has elapsed
- SQLite checkpoint/projection flush has completed or failed with a durable
  diagnostic

Managed controllers ignore idle exit and require explicit shutdown.

## Verification Plan

### Unit / store tests

- Singleton launch lock rejects concurrent controller launches.
- Session actor generation compare-and-swap rejects stale transitions.
- Same-generation lifecycle updates reject mismatched pane/session ids.
- SQLite transaction rollback leaves projections unchanged or reports drift.
- Lazy self-exit refuses to exit with active leases or in-flight dispatch.

### IPC / supervisor tests

- Supervisor can lazy-launch controller, detach it, disconnect, and reconnect.
- Two supervisors racing to start the same document produce one owner and one
  fail-closed response.
- A stuck `starting` supervisor is transitioned to `blocked` or replaced through
  an explicit new generation.
- `session clear` and route dispatch use the same authoritative submit boundary.

### Tmux-backed regressions

- Reproduce the `tsift.md` stale-starting shape: old owner generation is blocked
  or superseded explicitly; `Run Agent Doc` cannot silently no-op.
- Three visible/stashed panes for one document do not cause tmux-router to elect
  a new owner.
- Sync preserves layout when one visible file lacks a valid actor owner.
- Startup-miss marker clears only after newer actor/session provenance proves a
  real later open.
- Route returns stage-specific `accepted but unproven`, `owner busy`, `owner
  blocked`, or `stale generation` diagnostics instead of generic success.
- Route/sync latency logs attribute slow no-op paths to the specific phase
  (prune, ownership proof, tmux-router, dispatch proof, or cycle ack), and
  Phase D no-op safe-passive sync stays within the 1s total budget.

## Implementation Phases

### Phase A: controller shell and status path

- Add controller process entrypoint and singleton socket/launch lock.
- Add `agent-doc controller status` and internal connect-or-lazy-launch helper.
- Persist controller epoch/launch mode in SQLite or a small bootstrap record.
- No route/start behavior changes yet.

### Phase B: actor store behind controller

- Move current actor-record load/store operations behind controller commands.
- Dual-write projections from controller-owned state.
- Add drift diagnostics for `session-actors.json` and `sessions.json`.

### Phase C: supervisor registration and lifecycle reports

- Make Harness Supervisor startup register with the controller.
- Report prompt-ready, busy dispatch, waiting-input, halted, and closed through
  controller/actor transitions.
- Keep old logs as provenance output from those transitions.

### Phase D: route/start/sync read authority from controller

- Convert `start` and `route` to controller requests.
- Convert `sync` to consume controller bindings and treat tmux-router output as
  layout reconciliation only.
- Remove normal-path same-file pane stealing and session-log owner election.

### Phase E: operator and repair boundary

- Wire `session status/history/attach/restart/clear/doctor` to controller.
- Keep destructive recovery behind explicit repair/doctor `--repair`.
- Add clear, stage-specific plugin/editor diagnostics.

## Likely Files

- `src/agent-doc/src/session_actor.rs`
- `src/agent-doc/src/session_actor_cmd.rs`
- `src/agent-doc/src/supervisor/*`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/src/main.rs`
- `src/agent-doc/specs/08a-session-actor-contract.md`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `tasks/agent-doc/plan-single-owner-session-actor-transition.md`
- `tasks/agent-doc/plan-sqlite-session-store-concurrency.md`

## Exit Criteria

1. One Project Controller is the only live process allowed to create, replace,
   or close document owner generations.
2. One Session Actor is the only authority for a document session's generation,
   pane/window binding, harness id, and state.
3. Harness Supervisors are clients/workers; they report lifecycle facts but do
   not independently coordinate ownership.
4. SQLite persists enough state for controller restart recovery and for
   stage-specific diagnostics.
5. `sessions.json`, `session-actors.json`, tmux registry state, and session logs
   are projections/diagnostics on the normal path.
6. The `tsift.md` three-pane/stale-starting failure shape has tmux-backed
   regression coverage and fails closed with an actionable diagnostic.
