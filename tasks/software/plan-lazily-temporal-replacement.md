# Plan: Lazily durable runtime as a Temporal replacement

Status: exploratory; keep in the private `agent-loop` umbrella repository
pending the ownership/IP conversation and a build-versus-adapt decision.

## Goal

Build a Lazily-native durable execution host for workloads that would otherwise
use self-hosted Temporal. The target is application capability replacement, not
initial wire compatibility with Temporal's server or SDKs.

The first architecture uses:

- Lazily for reactive typed projection and exhaustive decisions;
- Postgres as the sole durable authority;
- NATS JetStream for leased ingress, wakeups, and publication transport;
- Valkey only for bypassable acceleration.

The detailed Postgres/NATS/Valkey adapter plan lives in
`plan-lazily-postgres-nats-valkey.md`.

## Product boundary

Keep the reusable pieces under the Lazily umbrella:

- `lazily-durable`: backend-neutral observations, decisions, effects, receipts,
  timers, leases, replay, and runner contracts;
- `lazily-postgres`: authoritative persistence, inbox, outbox, timers, leases,
  fencing, and migrations;
- `lazily-nats-jetstream`: async ingress, publication, wakeups, and dead-letter
  transport;
- `lazily-valkey`: optional cache, rate-limit, presence, and coalescing helpers;
- `lazily-spec`: language-neutral semantic and failure fixtures.

Keep a deployable control plane, operator UI, and workload-specific services in
a separate product repository. Lazily should not absorb deployment policy or
private application workflows.

## Authority invariant

```text
accepted ingress
    -> Postgres transaction
    -> durable observation + inbox claim
    -> Lazily reactive projection/table
    -> typed decisions
    -> state + timers + effects + outbox committed atomically
    -> transport work
    -> durable effect/publication receipts
    -> new Lazily observations
```

Postgres commit is the authoritative transition boundary. NATS acknowledgements
occur only after that commit. They report transport settlement; they never gate
or authorize the reactive projection. Valkey can be empty or unavailable
without changing correctness.

## Replacement scope

A credible replacement needs more than adapters. It needs explicit equivalents
for these Temporal capability families:

| Capability | Lazily replacement |
|---|---|
| Workflow History | append-only owner observations plus versioned snapshots in Postgres |
| Deterministic replay | shared fixtures and replay from canonical observations |
| Workflow Tasks | fenced owner runner evaluating a Lazily projection |
| Activity Tasks | typed effects with retry policy and durable receipts |
| Task Queues | JetStream durable consumers plus Postgres inbox/outbox authority |
| Durable timers | Postgres deadline store and leased timer dispatcher |
| Signals | idempotent asynchronous owner observations |
| Updates | validated request observation plus durable result correlation |
| Queries | read-only projection from an authoritative version |
| Child Workflows | parent/child owner links and correlated terminal receipts |
| Cancellation | propagated cancellation observations and effect policy |
| Retry/timeouts | explicit schedule, attempt, deadline, and terminal disposition |
| Continue-As-New | snapshot/compaction and new owner-run lineage |
| Worker versioning | behavior/schema version routing with replay gates |
| Visibility/search | derived read model, never transition authority |
| Schedules | durable schedule definitions producing idempotent starts |
| Operator controls | inspect, pause, retry, terminate, replay, and quarantine |
| Multi-region recovery | backup/restore first; replication only after semantics are proven |

Do not copy Temporal's API surface blindly. Prefer Lazily's smaller typed
observation/decision model where it can express the required semantics.

## Execution model

### Owner

An owner is the durable unit of serialization, comparable to one Workflow
Execution. It has:

- stable owner and run ids;
- behavior and codec versions;
- current fence token;
- canonical observation position;
- snapshot position and state hash;
- pending timer/effect/publication records;
- parent/child links;
- terminal disposition.

### Observation

Every accepted input is a versioned, idempotent observation:

- start;
- signal or correlated update request;
- timer fired;
- effect result/failure/cancellation;
- child result;
- cancellation;
- operator action;
- migration/version event.

Transport redelivery may repeat an envelope, but the inbox identity admits the
semantic observation at most once.

### Decision

The Lazily graph reactively projects the owner state and returns an exhaustive
typed decision set. A decision may:

- persist a state transition;
- schedule/cancel a timer;
- schedule/cancel an effect;
- publish via the outbox;
- start or notify a child owner;
- answer a correlated update;
- compact into a new run;
- complete, fail, cancel, or quarantine the owner.

Decision evaluation is pure. Effect execution begins only after the transaction
commits.

## Persistence model

Start with Postgres tables for:

- owners and runs;
- observations/events;
- snapshots;
- inbox deduplication;
- decisions/effects and receipts;
- timers;
- outbox and publication receipts;
- leases/fences;
- parent/child correlations;
- update request/results;
- schedules;
- visibility projections;
- dead-letter/quarantine dispositions.

Use optimistic version checks plus monotonic fencing. Persist state, new
observations, decisions, timers, effects, and outbox rows in one transaction.
Partition/retention design must preserve replay and audit requirements.

## NATS and Valkey

JetStream supplies scalable leased delivery, not durable workflow truth:

- commands/signals arrive through durable consumers;
- workers ACK only after the Postgres transition commits;
- outbox relays publish with stable message ids;
- wakeups indicate possible work but the database decides whether work exists;
- poison transport is reconciled to a durable Postgres disposition.

Valkey remains optional:

- safe: reconstructible read caches, rate limits, presence hints, single-flight,
  and wakeup coalescing;
- unsafe: workflow history, only-copy leases, inbox/outbox authority, timers, or
  transition receipts.

## Reliability requirements

- crash before commit: no accepted transition;
- crash after commit before ACK: redelivery resolves through inbox dedupe;
- stale worker: fence rejects its commit and publication claim;
- effect retry: stable effect identity prevents duplicate semantic application;
- NATS outage: accepted outbox work remains durable;
- Valkey outage/flush: latency may change, correctness does not;
- Postgres outage: stop accepting authoritative work;
- poison input: retain evidence and a terminal/quarantine disposition;
- clock skew: deadlines use persisted policy and database/monotonic boundaries,
  not unaudited worker wall clocks.

## Conformance strategy

Add language-neutral fixtures before broad implementation:

- all observation/decision variants;
- replay after every possible crash boundary;
- duplicate ingress and duplicate effect completion;
- timer cancellation/firing races;
- owner lease eviction and stale fencing;
- parent/child cancellation races;
- update correlation and timeout;
- snapshot/compaction equivalence;
- behavior-version migration;
- bounded backpressure and drain;
- outbox publish-before-receipt retry.

Rust should be the full reference host. Other languages earn capability marks by
replaying the same fixtures; they do not receive nominal adapter packages merely
for API shape.

## Migration from Temporal

Do not attempt transparent in-flight history import first.

1. Inventory workflows, Activities, Signals, Updates, Queries, timers, child
   workflows, retry policies, search attributes, schedules, and operator needs.
2. Classify each workload as adapter-only, replacement candidate, or unsuitable.
3. Re-express one completed-history workload as Lazily observations and verify
   decision parity offline.
4. Start new low-risk runs on the Lazily host while existing Temporal runs
   drain.
5. Bridge only necessary cross-system messages with stable correlation ids.
6. Never let both systems authorize the same run.
7. Cut over workload classes with rollback thresholds and audit reconciliation.
8. Consider in-flight migration only after a formal mapping and recovery proof.

## Delivery phases

### Phase 0 — decision and boundary

- Complete the ownership/IP review.
- Decide whether the adapter satisfies the actual need.
- Record required Temporal capabilities and explicit non-goals.
- Choose availability, recovery-point, recovery-time, scale, and retention
  targets.

### Phase 1 — single-node durable kernel

- Implement the observation/decision/effect model.
- Add Postgres events, snapshots, inbox/outbox, timers, receipts, and fences.
- Prove deterministic replay and all local crash boundaries.

### Phase 2 — distributed transport and workers

- Add JetStream ingress, wakeups, publication, and dead-letter handling.
- Run multiple fenced workers with bounded concurrency and graceful drain.
- Test redelivery, partitions, poison input, and rolling restart.

### Phase 3 — workflow capability families

- Add Updates/Queries, child owners, cancellation, schedules, compaction,
  version routing, and visibility.
- Add operator inspection, pause, retry, terminate, and quarantine.

### Phase 4 — production evidence

- Add backup/restore and disaster-recovery drills.
- Establish SLOs, capacity tests, security boundaries, and audit exports.
- Shadow candidate workloads and reconcile every accepted transition.

### Phase 5 — optional acceleration and portability

- Add Valkey only for measured bottlenecks.
- Port the semantic host to additional languages only through the conformance
  corpus.

## Build-versus-adapt gate

Default to the Temporal adapter unless replacement has a concrete advantage
that outweighs owning a durable execution platform.

Proceed with replacement only if:

- the required execution semantics fit Lazily's typed state-machine model;
- Temporal operating cost or constraints are measured, not hypothetical;
- the team accepts long-term ownership of persistence, recovery, versioning,
  security, observability, and operator tooling;
- conformance and disaster-recovery evidence meet the workload's risk level.

## Completion criteria

- [ ] Postgres alone can recover every accepted owner transition.
- [ ] Replay produces identical projected state and decisions.
- [ ] No broker ACK precedes the authoritative commit.
- [ ] Stale workers cannot commit or publish effects.
- [ ] Timers, retries, cancellation, children, updates, and compaction pass race
  fixtures.
- [ ] Backup/restore and rolling upgrades preserve accepted work.
- [ ] A migration can drain Temporal runs without dual authority.
- [ ] Operator tooling can explain and repair every non-terminal disposition.

## Public-source and ownership boundary

Temporal's open-source server and public architecture may be studied as public
prior art. Do not use employer-confidential workflow definitions, production
topology, incidents, internal extensions, benchmarks, or unpublished design
material. Keep this plan private until the ownership conversation is recorded;
publish reusable Lazily work only after that review.

