# Pane Execution Authority Topology

## Architecture Contract

### Invariant

A command may mutate a document's cycle, lease, retained-write, or closeout state only when no owner is registered, its explicit logical actor is the registered owner of the same actor generation, or exact process proof authorizes replacement of a proven-stale binding. A rejected command performs no mutation. Controller-owned effects use that explicit actor identity rather than the controller process's ambient `TMUX_PANE`.

### Policy owner

`agent-doc-state-backbone::pane_execution_authority` is the only policy owner. It exposes one exhaustive state table and one Lazily projection. I/O crates may collect observations and execute the returned action, but may not re-derive pane authority from registry strings.

### Inputs

- Invocation origin: headless process, foreground tmux pane, or controller effect carrying an explicit actor pane. An ambient `TMUX_PANE` inherited by a long-lived harness daemon is accepted only while tmux still proves that pane is live; otherwise resolution falls back to the attached client's current pane.
- Registered owner observation: absent, current logical actor, another logical actor, or indeterminate.
- Registered-owner liveness: live, stale, or indeterminate.
- Requested operation class: read-only probe or mutating command.

### Exhaustive decision table

| Operation | Invocation | Owner observation | Liveness | Verdict |
| --- | --- | --- | --- | --- |
| read-only | any | any | any | permit read-only |
| mutation | headless | absent | any | permit headless bootstrap |
| mutation | headless | present | any | reject unavailable invocation authority |
| mutation | explicit actor with exact process proof | same actor generation | live/indeterminate | permit owner |
| mutation | explicit actor without exact process proof | same pane string | any | reject; pane identity alone is not authority |
| mutation | explicit actor | different actor | live | reject live-owner mismatch |
| mutation | explicit actor with exact process proof | different actor | stale | permit proven stale-binding supersession |
| mutation | explicit actor without exact process proof | different actor | stale | reject and require typed stale-owner repair before retry |
| mutation | explicit actor | different actor | indeterminate | reject indeterminate authority |
| mutation | explicit actor | absent | any | permit unregistered document |
| mutation | pane unavailable | owner present | any | reject indeterminate authority |

The Rust table enumerates every constructible row from typed enums; tests fail if a variant is added without a row. Live-owner mismatch always dominates stale-repair advice, so recovery cannot seize a live session.

### Reactive boundary

Within one process/document lifetime, adapter observations are `Source`s and the verdict is one `Computed`. The typed verdict is the admission receipt consumed before any downstream cycle or write transition may run:

`invocation Source + owner Source + liveness Source + operation Source -> authority Computed -> typed admission receipt -> admitted mutation Effect`.

### Boundary ingress

Registry/actor changes enter through controller actor events. Tmux and process liveness enter through explicit command-admission observations. Controller RPC handlers bind the authoritative actor pane into the effect context. No timer or polling loop is introduced.

### Imperative extraction audit

- Remove the write runtime's direct `registry pane != ambient pane` policy branch.
- Keep document/session parsing, registry lookup, tmux lookup, and process-liveness checks as I/O adapters only.
- Validate ambient `TMUX_PANE` liveness at the I/O boundary before publishing invocation identity; keep the controller's typed actor override authoritative.
- Run the authority projection before preflight repair/recovery and before write mutation.
- Gate drain-owner lease acquisition through the same projection; lease release remains universally available for cleanup.
- Keep durable writes and tmux actions as effects gated by the authority verdict.
- Do not create per-call private Lazily contexts in controller-owned state; standalone contexts are allowed only in pure table tests.

### Allowed edit surfaces

- `agent-doc-state-backbone` for the typed table and Lazily projection.
- command/runtime adapter crates that collect authority observations or gate effects.
- controller RPC/actor dispatch code that supplies explicit actor identity.
- focused integration/property tests, the pane-authority TLA+ model, its runner, and topology documentation.

### Verification

- Exhaustive Rust table-coverage and truth-table tests.
- Adapter tests proving rejection precedes recovery, cycle-open, lease acquisition, and write mutation.
- Controller-effect tests proving ambient controller pane cannot change the explicit actor verdict.
- TLA+ invariants: non-owner never mutates, rejection has no side effects, at most one owner generation mutates, stale repair never seizes a live owner, and admitted recovery can reach terminal settlement under fairness.
- `make check`, private-name scan, install, and installed-binary smoke checks.

### Out of scope

- Redesigning tmux layout or session claiming UX.
- Proving arbitrary editor/CRDT content correctness outside pane execution authority.
- Automatically transferring a live session between panes.

## Supervisor Generation Transition Contract

### Incident evidence

`tasks/api.md` cycle `cycle-1790312196479` and `tasks/fpe.md` cycle
`cycle-1790310315710` were both durably `preflight_started` when the 2026-09-25
install fan-out marked their supervisors stale. At 05:00 UTC both supervisors
logged `supervisor_binary_stale_self_recycled boundary=idle`; neither cycle had
a response capture or another replay checkpoint. `api.md` subsequently retained
an authority/disk divergence and `fpe.md` remained interrupted at
`preflight_started`. These traces falsify the assumption that IPC drain plus a
prompt-derived idle boundary is sufficient to replace a supervisor mid-cycle.

### Invariant

A supervisor generation transition may not start while its document cycle is
open unless that exact cycle has a typed durable replay checkpoint. In
particular, a `preflight_started` cycle without a response capture is never
recyclable. Binary staleness, install fan-out, prompt visibility, elapsed watch
ticks, and drained supervisor IPC are observations, not replay proofs.

### Policy owner

`agent-doc-supervisor::lifecycle` owns the pure generation-transition decision.
The idle-watch adapter supplies cycle and durable-checkpoint observations and
executes the returned action; it may not weaken the cycle observation for a
stale binary or after a timeout.

### State table

| Cycle | Durable replay checkpoint | IPC checkpoint | Decision |
| --- | --- | --- | --- |
| closed | any | unsafe | defer unsafe checkpoint |
| closed | any | safe | apply the requested recycle policy |
| open | absent | any | defer open cycle |
| open | present | unsafe | defer unsafe checkpoint |
| open | present | safe | permit the typed recovery recycle only |

Install fan-out and ordinary stale-binary replacement never synthesize a replay
checkpoint. Write-wedge/editor-delivery recovery may assert one only after the
response capture and retry intent are durable.

### Liveness

Deferral is edge-driven, not timeout-driven. A normal cycle reaching
`committed` or `abandoned` re-evaluates the pending recycle and permits it. If
the child dies, boot recovery consumes the durable cycle/capture state; process
death, rather than a watch-tick count, is the authority to redispatch. A wedged
captured closeout retains its existing typed recovery edge.

### Verification

- Exhaustive pure-policy tests cover cycle open/closed, replay checkpoint
  absent/present, IPC safe/unsafe, and every recycle cause.
- Runtime source-order tests prohibit weakening `cycle_open` for stale/restart
  paths and prohibit elapsed-tick escalation.
- Simulator traces cover both incident shapes: install fan-out during an
  uncaptured preflight defers, then recycles only after terminal cycle state.
- A finite TLA+ model checks `OpenUncheckpointedNeverReplaced`,
  `UnsafeCheckpointNeverReplaced`, and eventual replacement after a terminal
  cycle under fairness.
