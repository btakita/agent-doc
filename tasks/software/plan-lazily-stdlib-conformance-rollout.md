# Plan: portable Lazily stdlib conformance and peer rollout

Status: Rust implementation shipped; portable contract, formal model, binding
ports, and feature scenarios pending.

Parent plans:

- `tasks/software/plan-lazily-interop-network-suite.md`
- `tasks/software/plan-lazily-interop-channel-matrix.md`
- `tasks/software/plan-lazily-interop-peer-rollout.md`

## Goal

Make the Lazily standard-library convenience layer a specified, formally
modeled, fixture-driven surface across Rust, Python, Kotlin, JavaScript, Dart,
Zig, Go, C++, and C#, then exercise every production implementation through the
single cross-binding peer suite.

Unsupported bindings remain visible as staged for each feature. They are
excluded from that feature's eligible peer group and receive no pass credit. A
feature with fewer than two eligible implementations reports
`insufficient_eligible_peers`; it must never become vacuously green.

## Scope and naming

This plan covers three portable concepts:

1. `Timer`: a monotone single-shot deadline with pending and latched fired
   observations.
2. `Timeout<T>`: a caller-driven adapter around an operation/future/wait
   primitive with strict monotone deadline semantics and typed `Completed`,
   `TimedOut`, `Cancelled`, and `Unavailable` outcomes.
3. `RevisionBarrier`: a monotone revision plus derived-predicate wait with
   deadline, cancellation, disposal/unavailable outcomes, and no check-to-sleep
   lost wakeup.

This is not the existing `resilience::TimeoutCell`. All nine bindings already
claim that reactive deadline-edge primitive through the resilience corpus. A
binding gets no stdlib credit from a similarly named `TimeoutCell`; it must
implement and test the caller-driven contract in this plan.

Implementations may use idiomatic language names, but these semantics are
mandatory:

- clocks are monotone and testable through an injected logical-time seam;
- a deadline poll at or after the boundary wins without polling operation or
  cancellation adapters;
- before the deadline, completion wins when completion and cancellation are
  observed in the same poll;
- the first terminal outcome is latched and repeated reads invoke no adapters;
- the adapter owns no hidden executor, mandatory async runtime, or thread;
- `RevisionBarrier` rechecks revision and predicate after registering its wake
  generation, so a racing advance cannot be lost;
- effect-receipt storage and transport remain application-owned.

## Authority and artifacts

The work lands in this order:

1. `src/lazily-spec/docs/stdlib.md` defines the language-neutral contract and
   explicitly distinguishes it from `docs/resilience.md`.
2. `src/lazily-spec/schemas/stdlib-fixture.schema.json` validates deterministic
   scenarios.
3. `src/lazily-spec/conformance/stdlib/` carries canonical Timer, Timeout, and
   RevisionBarrier fixtures.
4. `src/lazily-formal/LazilyFormal/Stdlib.lean` models the state machines and
   proves their safety laws.
5. Every binding replays the same corpus against its public implementation.
6. `src/lazily-interop` exposes versioned feature scenarios backed by those
   production APIs.

`src/lazily-spec/coverage.json`, generated coverage docs, and each binding's
README gain separate rows for:

- `stdlib_timer_v1`;
- `stdlib_timeout_v1`;
- `stdlib_revision_barrier_v1`.

A row turns green only after the binding's runner opens the canonical corpus,
asserts a positive fixture/assertion floor, and mutation checks prove the runner
observes library behavior.

## Canonical fixture corpus

All time is logical. A conformance runner must not sleep or depend on wall-clock
scheduling.

### Timer

- pending strictly before the deadline;
- fires exactly at the deadline;
- zero duration fires immediately;
- repeated reads preserve the original terminal result;
- monotone-clock regression is rejected or normalized according to the
  language-neutral contract;
- maximum-duration/overflow behavior is typed rather than wrapping.

### Timeout

- completion before the deadline;
- pending operation times out exactly at the deadline;
- pre-deadline completion wins a simultaneous cancellation observation;
- pending plus cancellation becomes `Cancelled`;
- zero duration does not poll operation or cancellation;
- an unavailable operation becomes `Unavailable`;
- repeated terminal reads call no supplied adapter;
- deterministic wait driving advances through a caller-owned clock/wait seam.

### RevisionBarrier

- already-satisfied revision and predicate return immediately;
- revision advances while the waiter crosses check-to-wait registration;
- predicate false then true after a later revision;
- exact-deadline timeout through the portable Timer contract;
- cancellation wakes a waiter with no deadline;
- disposal wakes all waiters and latches;
- foreign/unavailable cancellation input returns the typed unavailable result;
- keyed effect receipts remain external and cannot satisfy the barrier by
  themselves.

Each corpus family has at least one mutation that must fail when:

- the deadline comparison changes from `>=` to `>`;
- cancellation is checked before a pre-deadline completion;
- a terminal outcome is not latched;
- the barrier omits its post-registration recheck;
- a runner replaces the public implementation with fixture bookkeeping.

## Formal model

`LazilyFormal.Stdlib` defines pure transition systems over logical time:

- `TimerState = pending deadline | fired firedAt`;
- `TimeoutState α = pending deadline | completed α | timedOut | cancelled |
  unavailable`;
- `BarrierState = waiting revision generation | satisfied revision |
  timedOut | cancelled | disposed | unavailable`.

Required theorems:

- Timer and Timeout terminality are monotone;
- Timer cannot fire before its deadline and must fire at/after it;
- strict deadline dominance prevents an operation transition at the boundary;
- pre-deadline completion/cancellation resolution is deterministic;
- replaying a terminal observation is idempotent;
- barrier revisions never decrease;
- the registration/recheck transition has no lost-wakeup trace;
- disposal and cancellation cannot be overwritten by later advances;
- effect receipts are observational inputs, not barrier authority.

The Lean model is the semantic authority, not an implementation recipe. Binding
runners prove executable correspondence by replaying the canonical fixtures.

## Peer feature protocol

Eligible peers advertise only features backed by production library code:

```text
stdlib_timer_v1
stdlib_timeout_v1
stdlib_revision_barrier_v1
```

The peer contract adds channel-neutral `feature_reset`, `feature_step`, and
`feature_observe` operations. The adapter translates those controls into public
stdlib calls; it must not implement a second Timer, Timeout, or barrier state
machine.

The orchestrator selects peers independently for each feature:

- unsupported peers appear in `staged_peers` with a stable reason;
- eligible peers must agree with the canonical fixture oracle;
- fewer than two eligible peers is an unfulfilled/red feature row, not green;
- once three peers are eligible, every semantic scenario runs three-way so no
  implementation becomes the oracle;
- a peer that advertises a feature but cannot execute it fails closed.

The feature scenario then delivers completion, cancellation, revision-advance,
predicate-change, and disposal triggers across every production channel/codec
cell that the eligible peer advertises. The transported trigger is under test;
the resulting stdlib state remains local to the receiving implementation.
Claiming a channel while skipping a required feature trigger is red.

## Initial implementation matrix

| Binding | Current stdlib status | Initial peer status |
|---|---|---|
| Rust | Timer, Timeout, and RevisionBarrier shipped | eligible after canonical fixture runner and peer feature adapter land |
| Python | existing resilience TimeoutCell only; portable stdlib unverified | staged |
| Kotlin | existing resilience TimeoutCell only; portable stdlib unverified | staged |
| JavaScript | existing resilience TimeoutCell only; portable stdlib unverified | staged |
| Dart | existing resilience TimeoutCell only; portable stdlib unverified | staged |
| Zig | existing resilience TimeoutCell only; portable stdlib unverified | staged |
| Go | existing resilience TimeoutCell only; portable stdlib unverified | staged |
| C++ | existing resilience TimeoutCell only; portable stdlib unverified | staged |
| C# | semantic CRDT peer is active, but stdlib and executable channel adapters are unverified | staged for all three stdlib features |

This matrix is deliberately conservative. An audit may find reusable Timer or
barrier pieces, but no feature is advertised until the production surface and
canonical runner both pass.

## Execution phases

### Phase 0 — contract, fixtures, and proofs

- Land spec docs, schema, three fixture families, and coverage rows.
- Land the Lean model and required theorems.
- Add schema, fixture-count, assertion-floor, and mutation gates.
- Regenerate coverage without marking any binding green from documentation
  alone.

Gate: spec validation and Lean build are green; every mutation turns the
relevant runner red.

### Phase 1 — Rust reference implementation, not oracle

- Replay the canonical corpus against `lazily::stdlib`.
- Add peer feature controls that call the production Rust API.
- Compare Rust with an intentionally independent fixture interpreter before a
  second binding is eligible.

Gate: local corpus and mutations are green, but the cross-binding feature row
remains `insufficient_eligible_peers` until another production binding joins.

### Phase 2 — native ports

- Port Zig, Go, and C++ with deterministic clock/wait seams.
- Run race detection for Go and sanitizers for C++.
- Admit each feature independently; Timer support must not imply Timeout or
  RevisionBarrier support.

Gate: at least three production implementations pass every claimed feature,
including pairwise and three-way scenarios.

### Phase 3 — managed and dynamic ports

- Port Python, Kotlin, JavaScript, and Dart.
- Test sync and native async/future adapters without making an async runtime a
  package requirement.
- Exercise Node/browser and VM/browser channel carve-outs honestly.

Gate: eight bindings are green for all three features and every advertised
channel/codec feature cell.

### Phase 4 — C# staged promotion

- Add idiomatic .NET Timer, caller-driven Timeout, and RevisionBarrier public
  surfaces plus corpus runners.
- Keep C# staged per feature until those surfaces pass locally.
- Keep channel-trigger scenarios staged until executable .NET channel adapters
  are advertised; semantic CRDT participation does not grant channel credit.

Gate: C# is green as both trigger sender and receiver for every applicable
feature/channel cell, with no test-only DTO or alternate state machine.

### Phase 5 — CI and release

- PR: spec/formal gates, all local corpus runners, and deterministic spanning
  peer scenarios.
- Merge: every eligible binding as feature-trigger sender and receiver at least
  once per applicable channel/codec.
- Nightly: all eligible pairs, both directions, all channels, deterministic
  randomized schedules, and mutation artifacts.
- Release bindings only after their coverage rows and peer capability manifests
  match executable evidence.

## Completion criteria

- Spec docs, schemas, fixtures, coverage, and Lean laws are committed and green.
- All nine bindings implement and locally replay all three stdlib features.
- Every peer advertises exactly the features its production library supports.
- The single network suite runs every feature over every applicable advertised
  data channel and codec.
- Unsupported/staged cells are visible and never counted as passes.
- No feature row is green with fewer than two eligible peers, zero fixture
  opens, zero assertions, or zero transported triggers.
- Mutation tests prove the suite catches deadline, race-ordering, latching,
  lost-wakeup, and fake-runner defects.
- Per-binding releases are published only after inside-out commits and pinned
  superproject gitlinks are green.
