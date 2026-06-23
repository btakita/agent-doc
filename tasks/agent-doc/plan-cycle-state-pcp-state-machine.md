# Plan: cycle-state PCP state machine (`#c7j5`)

## Decision

Yes, moving cycle-state mutation into the Project Control Pane can improve
agent-doc cycle reliability now that the controller/supervisor/write-authority
work has advanced. The safe shape is not "delete the sidecar" or make closeout
depend on an always-available socket. The safe shape is:

- PCP/session actor owns the in-process mutation order.
- A lazily state machine owns the phase transition table and transition effects.
- The existing `.agent-doc/state/cycles/<hash>.json` sidecar remains the durable
  crash-recovery log/snapshot that every process can replay when the controller
  is absent, stale, or restarting.

This merges the sidecar logic into the PCP without turning the PCP into a single
point of failure.

## Current code facts

- `cycle_state.rs` is still the durable authority: public mutators load the JSON
  sidecar, mutate one field/phase, and save it atomically.
- `cycle_state::save` already uses atomic temp-file persist (`#lzsidecaratomic`),
  so torn reads are fixed.
- `#lzsidecarlock` still covers the remaining load/mutate/save lost-update
  window for sidecar-only writers.
- `project_controller::persist_session_actor_closeout` currently reads
  `cycle_state::load(file)` and writes a closeout projection into SQLite after
  the fact.
- `write_queue.rs` and `write_authority.rs` show the session-actor ordered write
  queue and visible-document write-authority cutover are already present, which
  makes a controller-owned cycle-state mutation path feasible.
- `agent-doc-orchestration` still consumes registry `lazily = "0.12.0"`, while
  the local `src/lazily-rs` checkout contains the newer `StateMachine`,
  `ThreadSafeStateMachine`, and `AsyncStateMachine` APIs. The dependency source
  must be resolved before using the new multi-context primitives.

## Target architecture

Add a controller/session-actor cycle-state layer:

1. `CycleEvent` is the only mutation input:
   `StartPreflight`, `ResponseCaptured`, `WriteApplied`, `Committed`,
   `Abandoned`, plus bookkeeping events for pending ids, queue heads, dropped
   prompts, semantic merge acks, and recovery flags.
2. `CyclePhaseMachine` wraps `ThreadSafeStateMachine<CyclePhase, CycleEvent>`
   inside the controller/session actor. Use `StateMachine` only for small
   single-threaded pure tests or `RunContext` caches; use `AsyncStateMachine`
   only if the controller path moves the state graph onto an async context.
3. Accepted transitions update the in-memory controller projection and append
   the durable sidecar snapshot in one serialized session-actor job.
4. Existing `cycle_state::*` public functions become compatibility wrappers:
   when the controller/session actor is available and the gate permits it, they
   submit a `CycleEvent`; otherwise they fall back to the current locked
   sidecar path.
5. The sidecar remains authoritative for crash replay. On controller startup,
   seed the state machine from the sidecar and reconcile with SQLite projection
   rows; sidecar beats stale memory.

## Migration gates

Use an explicit gate, for example `AGENT_DOC_CYCLE_STATE_AUTHORITY`:

- `sidecar`: current behavior, with `#lzsidecarlock` as the hardening floor.
- `shadow`: route events through the PCP state machine and write diagnostics,
  but keep sidecar mutators authoritative.
- `dual-write`: PCP accepts events and sidecar mutators still write; compare
  controller projection, sidecar JSON, and SQLite closeout projection after
  every transition.
- `controller`: public mutators submit events to the PCP/session actor first;
  sidecar writes are emitted from the owner job as the durable log.
- `removed`: delete only obsolete duplicate mutation code. Do not remove the
  sidecar durable log.

## Implementation slices

1. Dependency slice: make `agent-doc-orchestration` consume a lazily version that
   actually exposes the required state-machine APIs. Prefer a released crate;
   use a path/git override only if the release is intentionally part of the same
   delivery.
2. Spec slice: update `specs/08b-single-process-control-plane.md` with a
   "Cycle-state authority" section covering controller-owned mutation order,
   sidecar durability, startup replay, fallback, and rollback gates.
3. Pure FSM slice: add `cycle_state_machine.rs` with `CycleEvent`,
   transition-table tests, monotonic phase guards, and no filesystem effects.
4. Controller slice: add a per-document cycle-state machine to the session actor
   and expose a typed submit API. Seed from the sidecar on actor startup.
5. Compatibility slice: route `cycle_state::start_preflight`,
   `mark_response_captured`, `mark_write_applied`, `mark_committed`, and the
   high-risk `record_*` mutators through the gated controller submit path, with
   the current sidecar path as fallback.
6. Recovery slice: add replay/reconcile tests for controller restart,
   controller unavailable, stale SQLite projection, and sidecar-vs-memory drift.
7. Cutover slice: advance gates only after SimWorld plus live route-owned
   agent-doc closeout proves no response loss, no queue-head loss, and no
   cross-process lost update.

## Tests

- Pure transition table: legal/illegal `CycleEvent` transitions, stable
  duplicate committed events, abandoned terminal behavior.
- Concurrency: many concurrent bookkeeping events for one document land in a
  single deterministic order with no lost non-phase fields.
- Fallback: controller unavailable still uses the locked sidecar path and
  commits successfully.
- Replay: controller restart seeds from sidecar and does not overwrite a newer
  durable sidecar with stale memory.
- Projection: `persist_session_actor_closeout` sees the same final phase and
  queue-head data as the controller projection.
- Regression for the current risk: `mark_committed` racing a pending-id record
  preserves both `phase=Committed` and `pending_done_ids`.

## Verification

- `cargo fmt --check`
- focused `cargo test -p agent-doc-orchestration cycle_state`
- focused controller/session-actor tests
- `make check`
- dogfood build/install only at a safe recycle boundary
- latest `gh run list --workflow CI --limit 1` after push

## Notes

This should supersede the narrow "pilot lazily::StateMachine on RunContext
CyclePhase" framing. The better next step is a PCP/session-actor state machine
with the sidecar as the durable journal, not a RunContext-only cache and not a
controller-only source of truth.
