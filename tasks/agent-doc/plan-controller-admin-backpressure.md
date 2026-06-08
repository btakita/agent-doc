# Plan: controller-backed admin and backpressure controls

**Pending:** `#ctrlplane-adminbp`
**Status:** Draft
**Class:** `#agent-doc-bug` control-plane implementation plan

## Problem

The project controller is now the intended live authority for session actors,
generations, supervisor leases, and projection repair, but the operator control
surface still has gaps:

- `agent-doc admin list/detect` is useful for read-only inspection, but mutating
  actions still depend on per-command side effects and sidecar repair paths.
- Active auto-queues can make an otherwise simple operator command behave like a
  race with a running turn.
- Queue saturation and busy panes are reported as generic failures instead of
  typed backpressure decisions that a caller, editor, or auto-loop can handle.
- Manual handoff, stale actor reap, and projection repair need a shared
  generation-checked API instead of command-specific heuristics.

This plan extends the existing `#ipc-admin-api`, Project Controller MVP, and
`specs/08b-single-process-control-plane.md` contracts into the next executable
slice.

## Goal

Add controller-backed `AdminControl` and `QueueControl` operations that let the
CLI and editor integrations inspect and manage actor state without direct
sidecar edits.

Required capabilities:

1. Inspect one actor with lifecycle, supervisor lease, queue head, generation,
   recent dispatch/admin receipts, and projection lag.
2. Pause, resume, and drain queue work for one document or project.
3. Reject full queues with typed backpressure receipts.
4. Reap stale actors with compare-and-swap generation guards.
5. Force manual handoff only when the caller supplies the observed generation.
6. Trigger projection repair from committed controller state without changing
   actor ownership.

## Non-goals

- Do not make tmux-router an ownership authority.
- Do not replace the read-only dashboard with a new UI in this slice.
- Do not implement broad SimWorld race coverage here; that remains tracked by
  `#ctrlplane-simworld`.
- Do not silently kill or rebind the caller's own active actor without an
  explicit force guard.

## Design

### 1. Controller request model

Add typed controller requests that enter through the dispatch actor and record a
durable `admin_operations` or `dispatch_attempts` receipt before returning:

- `InspectActor { file | session_id | pane_id }`
- `QueueControl { scope, action, observed_generation?, item_id? }`
- `AdminControl { target, action, observed_generation?, reason, force? }`
- `ProjectionRepair { scope, projection, observed_generation?, reason }`

`scope` can be a document or the whole project. Mutating operations must include
the current observed actor generation when they target an actor. Missing or stale
generations return a typed rejection, not a best-effort mutation.

### 2. Queue control state

Represent queue pressure and operator queue state in SQLite, not as loose marker
files:

- `queue_heads`: document id, generation, head id/text, state, priority, updated
  timestamp.
- `queue_controls`: document/project scope, paused/draining/resumed state,
  reason, operator receipt id, timestamp.
- `queue_backpressure`: document id, generation, rejected command kind, capacity
  class, reason, receipt id, timestamp.

The existing `agent:queue` component and frontmatter remain document
projections. Closeout still mutates the markdown queue/backlog in the committed
cycle, but route/start/sync should ask the controller whether queue draining is
paused or under pressure before dispatch.

### 3. Typed backpressure

Add controller response variants for queue pressure:

- `Rejected { failed_stage: "queue_full", reason, receipt_id }`
- `Blocked { failed_stage: "queue_paused", unblock_hint, receipt_id }`
- `Blocked { failed_stage: "actor_busy_draining", unblock_hint, receipt_id }`

The CLI should print the typed reason and the exact command to inspect, resume,
or drain. Editor integrations should receive the same response over FFI/JSON so
the UI can avoid blind retry loops.

### 4. Admin mutations

Extend the admin CLI beyond read-only list/detect:

- `agent-doc admin inspect <document|--session <id>|--pane <pane>> [--json]`
- `agent-doc admin queue pause <document|--project-root ...> --reason <text>`
- `agent-doc admin queue resume <document|--project-root ...>`
- `agent-doc admin queue drain <document> [--until-id <id>]`
- `agent-doc admin reap <document|--session <id>|--pane <pane>>
  --observed-generation <n> --reason <text>`
- `agent-doc admin handoff <document> --to-pane <pane>
  --observed-generation <n> --reason <text>`
- `agent-doc admin repair-projection <document|--project-root ...>
  --projection sessions|actors|layout|all`

All mutations should fail closed when the target generation no longer matches.
`--force` may exist for repair-only cases, but the receipt must record that the
guard was overridden.

### 5. Projection repair

Projection repair reads committed controller state, then rewrites
`session-actors.json`, `sessions.json`, and layout projections from SQLite. It
must not infer ownership from the projection it is repairing.

Repair outcomes:

- `completed`: projection now matches committed state.
- `blocked`: state is ambiguous or a live supervisor lease needs operator
  decision.
- `rejected`: stale generation or unknown document/session.

### 6. Editor/FFI boundary

Keep editor plugins thin:

- expose FFI wrappers for inspect, queue pause/resume/drain, handoff, reap, and
  projection repair;
- return the same typed receipt envelope as the CLI;
- avoid plugin-specific ownership decisions.

## Implementation phases

### Phase 1 - controller data and receipts

- Add state-store schema and Rust types for queue controls, queue heads, and
  queue backpressure receipts.
- Add controller request/response types for `QueueControl`, `AdminControl`, and
  `ProjectionRepair`.
- Unit-test stale-generation rejection, accepted receipt durability, and typed
  backpressure serialization.

### Phase 2 - CLI admin commands

- Add `admin inspect`, `admin queue pause/resume/drain`, `admin reap`,
  `admin handoff`, and `admin repair-projection`.
- Route commands through the controller rather than mutating sidecars directly.
- Preserve existing `admin list/detect/dashboard` behavior.

### Phase 3 - queue integration

- Teach queue dispatch, preflight queue maintenance, and session clear/restart
  paths to consult controller queue state.
- Replace ad hoc busy-queue failures with typed `queue_paused`, `queue_full`, or
  `actor_busy_draining` receipts.
- Ensure closeout advances queue/backlog only in the same committed response
  cycle as before.

### Phase 4 - projection repair and FFI

- Implement projection repair from SQLite committed state.
- Add FFI/editor wrappers that return the typed receipt envelope.
- Update dashboard/status output to include queue pressure and projection lag.

## Tests

- Unit: stale observed generation rejects handoff/reap/queue mutation.
- Unit: accepted admin mutations write `admin_operations` receipts before
  external side effects.
- Unit: full queue returns typed `queue_full` with receipt id and retry hint.
- Unit: paused queue blocks dispatch without consuming queue head or backlog
  completion state.
- CLI: `admin inspect --json` includes actor state, generation, queue head,
  supervisor lease, and projection lag.
- CLI: `admin queue pause` blocks a follow-up queue dispatch; `resume` allows it.
- CLI: `admin repair-projection` rewrites missing/stale projections from SQLite
  and reports projection diagnostics.
- Regression: existing `admin list/detect/dashboard` tests remain green.

`#ctrlplane-simworld` should add the deterministic multi-actor race matrix after
these primitives exist.

## Verification

Focused checks:

```bash
cargo test -p agent-doc-orchestration project_controller -- --nocapture
cargo test -p agent-doc test_cli_admin -- --nocapture
```

Full closeout checks for implementation phases:

```bash
make check
make build-release install
agent-doc --version
gh run list --workflow CI --limit 1
```

## Exit criteria

1. Operators can inspect, pause, resume, drain, reap, handoff, and repair through
   controller-backed commands.
2. Every accepted or rejected admin/queue operation has a durable typed receipt.
3. Stale generation handoff/reap attempts fail closed.
4. Queue pressure is visible as typed backpressure, not a generic timeout.
5. Projection repair rewrites JSON/tmux-layout projections from SQLite state.
6. Editor integrations can use the same FFI/JSON envelope as the CLI.
