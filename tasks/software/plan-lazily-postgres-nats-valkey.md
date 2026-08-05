# Plan: Durable Lazily runtime with Postgres, NATS, and optional Valkey

**Status:** Proposed
**Created:** 2026-08-05
**Session:** `tasks/software/lazily.md`

## Goal

Extend Lazily from an in-process reactive state and scheduling library into a
durable distributed runtime without making storage or brokers transition
authorities.

The intended ownership split is:

- **Lazily** owns typed state projection, exhaustive decisions, retries,
  leases, and effect scheduling.
- **Postgres** is the durable system of record for accepted state, inbox
  deduplication, outbox publication, effect receipts, timers, and lease fences.
- **NATS JetStream** transports commands, observations, wakeups, and published
  events between processes.
- **Valkey** is optional disposable acceleration for caches, single-flight
  suppression, rate limiting, and short-lived coordination. Correctness must
  survive its complete loss.

## Architectural invariant

Every transition remains a reactive projection:

```text
external observation
        │
        ▼
adapter Source ──> decoded/deduplicated facts ──> Computed decision
                                                     │
                                                     ▼
                                               idempotent Effect
                                                     │
                                                     ▼
                                            durable effect receipt
                                                     │
                                                     └──> observed Source
```

There is no request/ACK state machine hidden in an editor, broker adapter, or
database adapter. An acknowledgement is only a transport receipt emitted after
the authoritative durable transition has committed.

## Project and package boundary

Lazily should remain the umbrella project because it owns the reactive
semantics, adapter contracts, and conformance rules. The integrations should not
be compiled into the core crate:

- `lazily-core` (the current core package): reactive graph, typed state tables,
  local effects, and backend-neutral identities;
- `lazily-durable`: durable-store, inbox/outbox, timer, lease, fencing, replay,
  and async-runner contracts plus conformance fixtures;
- `lazily-postgres`: Postgres schema, migrations, and durable adapter
  implementations;
- `lazily-nats-jetstream`: JetStream ingress, delivery leases, publisher,
  wakeups, and dead-letter integration;
- `lazily-valkey`: optional caches, rate limiting, and disposable coordination.

These can live as sibling packages in the Lazily repository initially, sharing
one CI matrix and compatibility policy. Split an adapter into its own repository
only when it needs independent ownership or a materially different release
cadence.

A deployable worker daemon, control plane, operator UI, and product-specific
workflow belong in a separate project. Haiven-specific orchestration, subjects,
schemas, and policies should consume the Lazily packages rather than entering
their public APIs.

## Cross-language rollout

Do not create Postgres, NATS, and Valkey packages for every supported language
merely for naming symmetry.

The initial reference implementation should be Rust:

- Rust owns the full durable runner, Postgres unit of work, JetStream delivery
  leases and relay, and optional Valkey acceleration.
- `lazily-spec` owns backend-neutral transition, crash, redelivery, fencing, and
  conformance fixtures.
- Other language ports claim explicit capability tiers rather than pretending
  that all runtimes have identical infrastructure support.

Suggested tiers:

| Tier | Capability |
|---|---|
| Core | Reactive graph and typed state/decision semantics |
| Client | Publish typed ingress and observe typed events through NATS |
| Durable host | Own Postgres transactions, inbox/outbox, timers, and fences |
| Distributed host | Add JetStream leased delivery, relay, drain, and recovery |
| Accelerated host | Add bypassable Valkey optimizations |

JavaScript, Python, Kotlin, C#, Go, and other ports may begin with a generated or
hand-written typed NATS client envelope. They should use the Rust host over a
stable wire protocol when they do not need to own transitions locally.

Add a native durable host only when all of these are true:

- a production service in that language must host the Lazily owner;
- mature Postgres and JetStream clients support the required transactional,
  backpressure, lease, and drain semantics;
- the shared conformance corpus can run against the implementation;
- maintainers accept its migration, compatibility, and failure-testing burden.

Valkey does not need a Lazily-specific package in a language that can already
use an ordinary Valkey client for a local, non-authoritative optimization. Add a
Lazily adapter only when it projects cache/coordination observations into the
graph and can prove the bypass invariant.

## IP and public-knowledge gate

The generic combination is public architectural knowledge:

- Lazily's published Rust package declares the MIT license.
- [Temporal Server is MIT-licensed](https://github.com/temporalio/temporal/blob/main/LICENSE).
- [NATS Server is Apache-2.0-licensed](https://github.com/nats-io/nats-server/blob/main/LICENSE).
- U.S. copyright law distinguishes protected implementation expression from
  unprotected ideas, systems, processes, and methods of operation
  ([17 U.S.C. § 102(b)](https://uscode.house.gov/view.xhtml?edition=prelim&num=0&req=granuleid%3AUSC-prelim-title17-section102%28a%29%282%29)).

That does not decide contractual ownership of a particular implementation.
Before employer-related implementation begins, obtain a written boundary that:

- identifies the existing Lazily repositories and releases as prior work;
- reserves generic Lazily adapters, conformance fixtures, and backend-neutral
  APIs for their existing open-source project;
- assigns product-specific workflows, private schemas, deployment
  configuration, and confidential business logic separately;
- states whether generic improvements may be contributed upstream;
- states the policy for employer time, equipment, credentials, and accounts;
- routes patentable or genuinely novel implementation work through the agreed
  disclosure process.

Keep employer-confidential examples, subjects, schemas, incidents, and code out
of public Lazily repositories even when the underlying architectural pattern is
public. This section is a project gate, not legal advice; the signed agreements
and applicable state law control.

## Required Lazily capabilities

### 1. Durable state and reducer protocol

Define a backend-neutral protocol for reconstructing and atomically advancing a
reactive owner:

- stable owner and workflow identity;
- monotonic state version or event position;
- serialized accepted state or ordered domain events;
- compare-and-swap transition fencing;
- idempotency key for every ingress item and effect;
- deterministic replay into `Source` observations;
- explicit schema and codec versions;
- durable effect receipts that feed the graph as facts.

The protocol must not expose SQL, NATS subjects, or Valkey keys in core state
machine APIs.

### 2. Async runner

The current local queue model is insufficient for network brokers. Add an async
runner that can:

- await multiple delivery, timer, and shutdown sources;
- bound concurrency per owner and globally;
- preserve per-owner ordering while allowing cross-owner parallelism;
- extend or surrender delivery leases;
- propagate cancellation and deadlines;
- apply retry/backoff policy without blocking a worker thread;
- drain cleanly during deployment;
- expose queue depth, age, attempts, and processing latency.

### 3. Transactional effect boundary

One unit of work must atomically persist:

- inbox/deduplication record;
- accepted state or domain events;
- effect intents in an outbox;
- timer changes;
- lease/fence updates;
- successful effect receipts when the effect is local to the transaction.

External publication happens after commit. Its publication receipt is then
recorded idempotently and projected back into Lazily.

## Postgres adapter

Postgres is the first required integration because it supplies durable
authority and the atomic boundary that a broker alone cannot provide.

### Adapter surfaces

- `DurableStateStore`
  - load an owner at a consistent version;
  - append events or replace a snapshot with optimistic fencing;
  - replay from a durable position.
- `TransactionalInbox`
  - claim an ingress idempotency key;
  - distinguish new, in-progress, completed, and poisoned deliveries.
- `TransactionalOutbox`
  - append publications in the same transaction as the state transition;
  - claim batches with `FOR UPDATE SKIP LOCKED`;
  - record publication attempts and terminal receipts.
- `DurableTimerStore`
  - store deadlines and retry state;
  - claim due work with lease fencing.
- `LeaseStore`
  - acquire, renew, and release owner leases with monotonic fence tokens.

### Postgres requirements

- migration-owned schema with forward-compatible codec versions;
- connection-pool backpressure;
- transaction retry classification;
- advisory-lock avoidance unless a measured use case requires it;
- retention and compaction policy for inbox, outbox, events, and receipts;
- replica-read rules that never let stale reads authorize a transition;
- test fixtures for rollback, serialization conflicts, crash after commit, and
  relay replay.

## NATS JetStream adapter

JetStream should be an asynchronous leased-delivery adapter family, not an
implementation of the existing synchronous local `QueueStorage` interface.

### Adapter surfaces

- `JetStreamIngress`
  - bind a stream and durable consumer to a typed codec;
  - expose message identity, subject, headers, delivery count, and deadline;
  - produce a delivery lease with `ack`, `nak`, progress, and terminal routing.
- `JetStreamPublisher`
  - publish outbox rows with a stable message id;
  - record the broker publication acknowledgement;
  - support subject mapping and versioned envelopes.
- `JetStreamWakeupSource`
  - wake an owner or scheduler without becoming the source of truth for whether
    work exists.
- `JetStreamDeadLetterSink`
  - publish terminally invalid or exhausted deliveries with diagnostic context
    while retaining their Postgres disposition.

### Processing order

```text
JetStream delivery
  -> decode and validate
  -> begin Postgres transaction
  -> claim inbox idempotency key
  -> load fenced owner state
  -> run Lazily projection/table
  -> persist state + outbox + timers + receipts
  -> commit Postgres
  -> ACK JetStream delivery
  -> relay outbox publications
  -> record publication receipts
```

The database commit precedes the delivery ACK. A crash between them causes
redelivery, which the inbox record resolves idempotently. A crash after an
outbox publish but before its receipt is also safe because the publication uses
a stable message id and the relay is retryable.

### NATS-specific policy

- one durable consumer policy per workload class, not per process;
- explicit `AckWait` and redelivery budgets derived from processing latency;
- progress acknowledgements only extend transport ownership; they do not
  authorize state transitions;
- bounded `MaxAckPending` aligned with database and runner capacity;
- deterministic subject and stream naming;
- poison messages receive a durable Postgres disposition before terminal
  broker handling;
- broker unavailability pauses ingress or outbox relay without corrupting the
  accepted state.

## Valkey adapter

Valkey is beneficial only after the Postgres and JetStream path is correct and
measured. It should remain optional.

### Good uses

- read-through caches of reconstructible projections;
- single-flight suppression for expensive reads;
- token-bucket rate limiting;
- short-lived presence and liveness hints;
- best-effort wakeup coalescing;
- disposable routing hints;
- observability counters where loss is acceptable.

### Prohibited authority

Do not use Valkey as:

- workflow or state-machine authority;
- the only copy of a lease or fence token;
- the only inbox/outbox or effect receipt store;
- a second durable event log;
- a duplicate JetStream transport via Valkey Streams;
- evidence that a transition committed.

Every Valkey-backed optimization needs a bypass path and a test proving the
system remains correct when Valkey is empty, stale, partitioned, or unavailable.

## Failure semantics

| Failure point | Required result |
|---|---|
| Before Postgres commit | No accepted transition; NATS may redeliver |
| After commit, before NATS ACK | Redelivery resolves through durable inbox deduplication |
| After outbox claim, before publish | Lease expires and another relay retries |
| After publish, before receipt | Stable publication id makes retry idempotent |
| Postgres unavailable | Stop accepting authoritative work; do not ACK |
| NATS unavailable | Accepted state and outbox remain durable; relay resumes later |
| Valkey unavailable or flushed | Performance may degrade; correctness is unchanged |
| Worker loses owner lease | Fence rejects stale commit or effect publication |

## Delivery phases

### Phase 0 — contracts and invariants

- Define backend-neutral durable-store, async-delivery, publication, timer, and
  lease traits.
- Specify idempotency, fencing, ACK ordering, and effect-receipt semantics.
- Add conformance fixtures before implementing adapters.
- Review dependency licenses and repository boundaries before importing code.

### Phase 1 — Postgres authority

- Implement migrations and the Postgres state, inbox, outbox, timer, and lease
  adapters.
- Add transactional unit-of-work support to the runner.
- Prove replay and crash recovery with deterministic integration tests.
- Operate initially with the local queue or direct test ingress.

### Phase 2 — JetStream transport

- Implement typed ingress, delivery lease, publisher, wakeup, and dead-letter
  adapters.
- Add the outbox relay.
- Prove commit-before-ACK, redelivery deduplication, backpressure, and drain
  behavior against a real NATS test service.

### Phase 3 — integrated distributed runner

- Run multiple workers against shared Postgres and JetStream.
- Add owner-affinity scheduling, lease renewal, cancellation, and bounded
  concurrency.
- Exercise worker crashes, network partitions, rolling deploys, and poison
  messages.

### Phase 4 — operations and cutover

- Add metrics, traces, structured failure dispositions, and operator tooling.
- Define retention, compaction, backup, and restore procedures.
- Shadow or dual-read existing workloads without dual-authorizing transitions.
- Cut over one workload class at a time with rollback criteria.

### Phase 5 — optional Valkey acceleration

- Add only optimizations justified by profiling.
- Ship each behind a bypassable feature/configuration gate.
- Chaos-test flushes, stale values, failover, and full unavailability.

## Completion criteria

- [ ] Every accepted transition is recoverable from Postgres alone.
- [ ] Every ingress delivery is idempotent across crash and redelivery.
- [ ] No JetStream ACK occurs before the corresponding durable commit.
- [ ] Outbox publication is retryable without duplicate semantic effects.
- [ ] Stale workers are rejected by a durable fence.
- [ ] Backpressure bounds memory, broker delivery, and database concurrency.
- [ ] Multi-worker integration tests cover restart, partition, and rolling drain.
- [ ] Valkey can be disabled or flushed without changing correctness.
- [ ] Core Lazily APIs remain backend-neutral and preserve reactive projection.

## Non-goals

- Replacing Lazily's graph with Postgres triggers, NATS consumers, or Valkey
  scripts.
- Treating transport acknowledgements as application decisions.
- Exactly-once transport claims; the target is idempotent at-least-once
  processing with durable receipts.
- Adding Valkey merely because it is available.
- Building a second scheduler or state machine inside an adapter.
