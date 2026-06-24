# State Backbone

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

The cycle FSM must not directly encode queue ordering, editor ACK shapes,
supervisor restart policy, route prompt readiness, or proof-specific transport
facts. Those are inputs to the cycle closeout decision, not cycle phases.

## Event Ledger

Every meaningful state transition should have a typed fact that can be replayed.
Examples:

- `PreflightStarted`
- `BaselineSaved`
- `QueueHeadSelected`
- `QueueHeadCompleted`
- `ResponseCaptured`
- `EditorAckObserved`
- `IpcProofInsufficient`
- `CommitObserved`
- `SessionCheckPassed`
- `AgentRestartPerformed`
- `CapabilityProofObserved`
- `ActorGenerationChanged`

Events must carry stable ids where available: document hash, session id, cycle
id, actor generation, patch id, queue node key, backlog id, and causation id.
The event log is append-only. Corrections are new events that supersede earlier
facts by projection rules; they are not in-place mutation of old facts.

## Projections

Current state is derived by deterministic reducers over the event ledger and the
document components. Required projections include:

| Projection | Owns |
|---|---|
| Document projection | current component bodies, boundaries, snapshot relation, prompt-bearing tail |
| Queue projection | active head, struck/completed heads, drainability, backlog-to-queue sync |
| Closeout projection | latest cycle phase, captured response materialization, pending write boundary |
| Transport projection | socket/file IPC patch state, ACK proof, retry/no-disk fallback state |
| Supervisor projection | actor state, child pid, harness, capability proof, restart epoch |
| Route projection | authoritative pane, readiness, dispatch authorization, dispatch proof |
| Proof projection | ops-log markers, typed verify/disproof predicates, semantic completion advisories |

Reducers must be idempotent and replay-testable. If two modules need the same
answer, they should consume the same projection instead of recomputing from
free-form logs or partial document text.

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

## Where Other Models Fit

| Model | Fit | Use |
|---|---|---|
| FSM | Strong local fit | small closed state sets: cycle phase, transport connection, proof gate, recycle lifecycle |
| Behavior tree | Policy helper only | inspectable recovery/dispatch priority and fallback logic that reads projections and emits commands |
| GOAP | Poor durable-state fit | avoid for correctness-critical closeout; planning machinery is harder to replay than reducers |
| Coroutine | Good protocol expression | linear handshakes such as writeback, startup proof wait, or clean-exit restart; persist checkpoints as events |
| Event-driven | Strong backbone fit when typed | append-only events, causation ids, idempotent reducers, replay tests |
| MPC | Not a fit | agent-doc state is discrete workflow convergence, not continuous control optimization |

## Regression Rule

New state-bearing behavior must answer two questions in the same change:

- Which owner emits the typed event?
- Which projection reduces it into current state?

Adding another tactical `reason=...`, `proof=...`, ad hoc boolean, or log-only
string in a hot path is not enough unless the change also explains why that fact
is intentionally not part of the state backbone.
