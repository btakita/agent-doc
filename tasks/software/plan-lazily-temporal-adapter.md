# Plan: Lazily adapter for self-hosted Temporal

Status: planning; keep in the private `agent-loop` umbrella repository pending
the ownership/IP conversation.

## Goal

Use Lazily as the deterministic projection and decision layer inside Temporal
Workflows while Temporal remains the sole durable workflow authority.

This is an adapter, not a second workflow engine. Temporal owns Workflow
History, task dispatch, durable timers, Activity retry state, cancellation, and
workflow lifecycle. Lazily reconstructs typed derived state from that history
and produces exhaustive decisions that the Workflow maps to Temporal commands.

## Authority invariant

For a Temporal-hosted owner:

```text
Temporal History
    -> typed workflow observations
    -> Lazily Sources
    -> deterministic Lazily projection/table
    -> typed decisions
    -> Temporal timers / Activities / child Workflows / completion
    -> resulting Temporal History events
```

There is no request-plus-editor-style acknowledgement in this loop. A new
Temporal observation changes a Source, the Lazily graph reactively projects a
decision, and the Workflow emits the corresponding command. Activity and
transport acknowledgements return later as new observations; they do not
authorize the original decision.

Temporal History is the only durable authority for this mode:

- do not mirror authoritative workflow state into Lazily Postgres tables;
- do not let NATS acknowledgements authorize workflow transitions;
- do not perform network or database I/O from deterministic Workflow code;
- do not treat Valkey state as evidence that a Workflow transition occurred.

## Adapter surface

Create a backend-neutral `lazily-temporal` contract and implement it only in
languages where both the Lazily binding and Temporal SDK support the required
semantics.

### Inputs

Normalize these events into versioned Lazily observation types:

- Workflow start arguments;
- Signals;
- synchronous validated Updates;
- Activity completion, failure, cancellation, and heartbeat-derived status;
- timer firing and cancellation;
- child Workflow completion or failure;
- cancellation and termination intent visible to Workflow code;
- Continue-As-New carry state.

Each observation needs a stable semantic identity so replay cannot duplicate a
logical transition.

### Projection host

The host constructs the same Lazily graph on every Workflow replay and applies
observations in deterministic order. It exposes:

- current projected state;
- a typed, exhaustive decision table;
- pending command identities;
- receipts already observed in history;
- a serializable Continue-As-New snapshot;
- read-only query projections.

The graph must not depend on wall-clock time, random values, process identity,
thread scheduling, unordered collection iteration, or external I/O. Such values
must come from Temporal APIs or recorded observations.

### Outputs

Map decisions to Temporal commands:

- schedule or cancel a timer;
- schedule, cancel, or await an Activity;
- start, signal, update, cancel, or await a child Workflow;
- Continue-As-New;
- complete or fail the Workflow.

Assign stable command/effect keys. The adapter must distinguish:

- decided but not yet represented by a history event;
- scheduled/accepted by Temporal;
- completed with a recorded result;
- terminally failed or cancelled.

Do not claim exactly-once external effects. Activities remain idempotent or
explicitly non-retryable according to their effect contract.

## NATS, Postgres, and Valkey boundaries

NATS may be used outside Workflow code:

- a NATS consumer validates a message and Signals or Updates the Workflow;
- an Activity publishes to NATS using a stable message id;
- publication results return through the Activity result recorded in history.

Postgres and Valkey may be used by Activities or surrounding services, but not
as co-authorities for the Workflow:

- Postgres can hold application records and Activity idempotency receipts;
- Valkey can cache reconstructible reads or provide disposable rate/presence
  hints;
- neither database determines whether the Workflow advanced.

If the same workload instead chooses Postgres as workflow authority, it leaves
this adapter mode and uses the Temporal-replacement architecture.

## Replay and versioning

- Add a canonical replay corpus for every observation and decision variant.
- Replay recorded histories in CI.
- Require deterministic ordering for maps, sets, subscriptions, and emitted
  commands.
- Version observation codecs and decision schemas.
- Use Temporal-compatible Workflow versioning practices for behavior changes.
- Define a history-size budget and Continue-As-New threshold.
- Carry only canonical inputs and compact projected state across
  Continue-As-New; never carry process-local caches.

## Package strategy

Do not create nine nominal adapters immediately.

1. Define the semantic fixtures in `lazily-spec`.
2. Build one reference adapter in the best-supported Lazily/Temporal language.
3. Add another language only when its Temporal SDK can replay the shared corpus.
4. Mark unsupported bindings explicitly rather than shipping façade packages.

The contract should be portable, but SDK-specific worker integration belongs in
each language package. A small cross-language envelope specification is shared;
Workflow runtime code is not.

## Delivery phases

### Phase 0 — contract

- Specify the authority invariant and typed observation/decision algebra.
- Inventory the selected Temporal SDK's replay, versioning, Update, timer,
  Activity, child Workflow, and Continue-As-New APIs.
- Add architecture tests that reject network access from Workflow projection.

### Phase 1 — deterministic reference host

- Implement start, Signal/Update, timer, Activity, cancellation, and completion
  mappings.
- Add query projection and stable command identities.
- Replay every fixture twice and compare decisions byte-for-byte.

### Phase 2 — external integration

- Add NATS-to-Signal/Update ingress and Activity-based NATS publication.
- Add Activity idempotency receipts where external effects require them.
- Exercise duplicate delivery, Activity retry, worker crash, and replay.

### Phase 3 — operations

- Add history-size and Continue-As-New policies.
- Add worker versioning and rolling-deploy tests.
- Add metrics for projection decisions, command latency, replay failures, and
  poison observations.

### Phase 4 — additional languages

- Port only from the shared semantic corpus.
- Publish a capability table that distinguishes full replay conformance,
  partial support, and unsupported SDK features.

## Completion criteria

- [ ] Temporal History is the sole durable workflow authority.
- [ ] Workflow replay produces identical Lazily state and decisions.
- [ ] Every external effect crosses a Temporal Activity boundary.
- [ ] NATS ingress and publication survive duplicates and retries.
- [ ] Queries cannot mutate projected state.
- [ ] Continue-As-New preserves semantic state without unbounded history.
- [ ] A rolling worker upgrade passes recorded-history replay tests.
- [ ] At least one language adapter passes the complete shared corpus.

## Public-source and ownership boundary

Temporal's public server is MIT-licensed, and its public architecture describes
event-sourced Workflow History, deterministic Workflow code, Activities, and
self-hosting. Those concepts can inform this plan. Implementation work must
still avoid employer-confidential workflows, schemas, incidents, benchmarks,
or unpublished design material. Record the ownership discussion before moving
this plan or derived code into a public Lazily repository.

