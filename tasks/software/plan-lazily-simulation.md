# Plan: Lazily deterministic simulation and property testing

**Status:** Apache-2.0 selected by the owner; repository metadata, dependency
inventory, and fixture provenance gates implemented in `lazily-spec`
**Created:** 2026-09-25
**Session:** `tasks/software/lazily.md`
**Backlog:** `#lzsimipgate` and `#lzsimlicense` completed

## Goal

Define the public-pattern, dependency-license, fixture-provenance, and ownership
boundary for Lazily's deterministic simulation work before implementation. This
gate covers the backend-neutral contracts, Go `SimWorld`, generation and
shrinking, fault plans, oracles, and eventual consumer testkit.

The gate separates learning from public ideas from copying protected expression.
Lazily may implement the abstract testing techniques below, but its APIs, type
names, schemas, traces, examples, fixtures, and implementation must be authored
independently from Lazily's own semantics.

## Gate decision

- Public-source research and private architectural planning may proceed.
- Simulation contracts and fixtures may be published through `lazily-spec`
  under Apache-2.0 when they satisfy the enforced fixture provenance policy.
- `lazily-go` implementation must remain independently authored and stdlib-only
  unless a later review adds a dependency to this inventory before its use.
- Do not begin consumer- or employer-related simulation work until the written
  prior-work/employer boundary below is either linked by date and location or
  explicitly marked not applicable by the owner. Never place confidential
  agreement text in a public repository.
- A later phase may proceed only after its reviewer confirms that the applicable
  checklist items in this plan are complete. Recording this gate does not itself
  waive an open condition.

## Public-pattern provenance register

These sources are conceptual references, not dependencies or sources to port.

| Pattern | Primary or official source | Permitted learning | Independent-authorship boundary |
| --- | --- | --- | --- |
| Deterministic whole-system simulation | FoundationDB, [Simulation and Testing](https://apple.github.io/foundationdb/testing.html); source repository is [Apache-2.0](https://github.com/apple/foundationdb/blob/main/LICENSE) | A seeded, deterministic, single-threaded simulated world; logical time; reproducible schedules; injected machine, network, and storage faults | Do not copy Flow/runtime code, APIs, configuration vocabulary, fixture structure, test workloads, or implementation organization. Do not present Lazily as FoundationDB-compatible. |
| Property generation and shrinking | Claessen and Hughes, [QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs](https://doi.org/10.1145/351240.351266); official [`Arbitrary` API](https://hackage.haskell.org/package/QuickCheck/docs/Test-QuickCheck-Arbitrary.html); implementation is [BSD-3-Clause](https://github.com/nick8325/quickcheck/blob/master/LICENSE) | State-dependent generators, executable properties, and ordered simplification of failing inputs | Design Lazily generators and causal shrinkers independently. Do not copy combinator APIs, implementation code, examples, names, or fixtures. |
| Go fuzzing | Official [Go fuzzing documentation](https://go.dev/doc/security/fuzz/) and [tutorial](https://go.dev/doc/tutorial/fuzz); Go code/toolchain is [BSD-3-Clause](https://github.com/golang/go/blob/master/LICENSE) and site documentation terms are described by [go.dev copyright](https://go.dev/copyright) | `testing.F`, seed corpora, coverage-guided mutation, minimized failures, deterministic targets, and retained regression inputs | Use the standard toolchain API, but author all seeds, encodings, assertions, and examples independently. Do not copy documentation examples into the Lazily corpus. |
| Linearizability checking | Herlihy and Wing, [Linearizability: A Correctness Condition for Concurrent Objects](https://doi.org/10.1145/78969.78972) and the [author-hosted paper](https://www.cs.cmu.edu/~wing/publications/HerlihyWing90.pdf) | Invocation/response histories, legal sequential explanations, and preservation of real-time precedence | The paper is citation-only. Do not copy prose, figures, pseudocode, notation-heavy examples, or histories. Apply the criterion only to APIs that claim linearizability. |

The register records the ideas used and enough provenance to review them. It does
not import the sources' code licenses into independently authored Lazily code.
Any later code, data, or fixture import requires a separate row with its exact
version or commit, license, notice obligations, intended use, and approval.

## Dependency license inventory

### `lazily-go`

- Repository license: MIT, declared by `src/lazily-go/LICENSE`.
- Module: `github.com/lazily-hub/lazily-go`, Go 1.24 floor.
- `go.mod` has no `require` directives; `go list -m all` reports only the Lazily
  module.
- Planned simulation dependency posture: Go standard library and built-in
  `testing` fuzzing only. The Go toolchain and standard library are BSD-3-Clause.
- FoundationDB and QuickCheck are reference-only. They are not linked, vendored,
  translated, or runtime/test dependencies.

### `lazily-spec` development tooling

`lazily-spec` is a private/dev-only Python package for schema and corpus checks.
The owner selected Apache-2.0 for the Lazily package family; `lazily-spec`
declares it in `LICENSE` and `pyproject.toml`, with `NOTICE` preserving the
project attribution. `third-party/uv-lock-licenses.json` is generated from the
exact `uv.lock` package set and checked by `make check`; a new or removed locked
package requires an explicit SPDX/source review before the inventory regenerates.
Its direct development dependencies are `jsonschema`, `referencing`, and
`pytest`; `uv.lock` resolves the following complete package set as of this gate:

| Package | Locked version | SPDX/license | Use |
| --- | ---: | --- | --- |
| attrs | 26.1.0 | MIT | `jsonschema`/`referencing` transitive dependency |
| colorama | 0.4.6 | [BSD-3-Clause](https://github.com/tartley/colorama/blob/master/LICENSE.txt) | Windows-only `pytest` transitive dependency |
| iniconfig | 2.3.0 | MIT | `pytest` transitive dependency |
| jsonschema | 4.26.0 | MIT | direct development schema validation |
| jsonschema-specifications | 2025.9.1 | MIT | `jsonschema` transitive dependency |
| packaging | 26.2 | Apache-2.0 OR BSD-2-Clause | `pytest` transitive dependency |
| pluggy | 1.6.0 | MIT | `pytest` transitive dependency |
| Pygments | 2.20.0 | BSD-2-Clause | `pytest` transitive dependency |
| pytest | 9.1.1 | MIT | direct development test runner |
| referencing | 0.37.0 | MIT | direct development JSON reference handling |
| rpds-py | 2026.6.3 | MIT | `jsonschema`/`referencing` transitive dependency |
| typing-extensions | 4.15.0 | PSF-2.0 | Python-version-conditional transitive dependency |

Versions and license expressions were checked against the versioned PyPI JSON
metadata (`https://pypi.org/pypi/<name>/<version>/json`); Colorama's incomplete
PyPI license field was resolved from its upstream license file. The Lean toolchain
is pinned to Lean 4 v4.30.0 and `lake-manifest.json` declares no Lean packages.
The existing fixture-generator scripts use only Python or Node standard-library
imports.

Before adding a dependency, update this table with its exact version/commit,
SPDX expression, source, notice/attribution requirements, purpose, and approval.
The public repository's generated inventory is authoritative; this table is the
historical intake record.

## Fixture provenance and license inventory

### Current corpus

- `src/lazily-spec/conformance/` contains 160 tracked JSON fixtures. Repository
  history attributes the corpus and its local generators to Brian Takita, and
  the audit found no external-source URLs in the corpus or generator scripts.
  That is evidence of first-party authorship, not a substitute for a license.
- Only three fixtures currently declare a top-level `generator` field:
  `codec/blob_backend_discriminator.json`, `codec/nodeid_exact_range.json`, and
  `codec/nodekey_null_leniency.json`. Seven generated message-passing fixtures
  do not declare their generator. No fixture declares a license, copyright,
  origin, or external provenance field.
- `src/lazily-go/test/conformance/` contains nine vendored JSON fixtures, all
  byte-identical to their canonical `lazily-spec` counterparts. The provenance
  ledger now records each copy's canonical path, digest, and Apache-2.0 source
  license in addition to the existing drift/sync checks.
- No external simulation corpus has been imported.
- `src/lazily-spec/provenance/fixtures.json` records every canonical fixture and
  all nine byte-identical Go copies by path, SHA-256, origin, and Apache-2.0
  license. Its generator is part of `make check`, so additions, deletions, byte
  drift, and unreviewed vendored copies fail closed.

### License decision and reuse terms

On 2026-09-25 the owner selected Apache License 2.0 for the Lazily package
family. `lazily-spec` now carries the full license, matching project metadata,
NOTICE, the generated locked-dependency inventory, and exact fixture provenance
ledger. Canonical specifications, schemas, and fixtures may be reused under
Apache-2.0 unless a file says otherwise. Vendored copies preserve that source
license and canonical origin.

New generated simulation fixtures must live under `conformance/simulation/` and
declare `schema_version`, `origin`, `license`, `seed`, and generator path/version.
Imported material additionally requires exact upstream identity, license,
notices, and reviewer approval. The generator rejects missing metadata before
the provenance ledger can be updated.

## Independent-authorship rules

1. Derive only abstract requirements from the public references. Record the
   source, access date, concept used, and reviewer before implementation.
2. Design Lazily names, types, schemas, scheduling rules, trace encoding, and
   example domains from Lazily's existing `ManualClock`, `StateMachine`, and
   `ReplayHarness` semantics. Do not perform source-to-source ports.
3. Use synthetic, generic domains and independently authored data. A simulator
   must call the same pure reducers or decisions as production through narrow
   generic ports; it must not recreate consumer behavior in a test-only model.
4. Do not copy third-party code, tests, configuration, fixtures, corpus bytes,
   prose, diagrams, example histories, or API shape. Similar behavior is not
   permission to reuse expression.
5. Every generated fixture records its generator path/version, seed, schema
   version, and origin. Every imported artifact additionally records its exact
   upstream version/commit, license, preserved notices, and reviewer approval.
6. Keep a reviewable commit trail. If provenance is uncertain, stop and replace
   the material with a clean, independently authored equivalent or obtain an
   explicit license before continuing.

## Excluded material

Do not place any of the following in Lazily simulation code, plans intended for
publication, traces, fixtures, examples, commits, or history:

- consumer, customer, or employer names and identifiers;
- workflows, business logic, policies, rules, schemas, subjects, or event names;
- production topology, configuration, credentials, accounts, or data;
- incidents, postmortems, tickets, logs, traces, traffic captures, recordings,
  transcripts, benchmarks, or performance results;
- internal extensions, unpublished designs, code, tests, or fixtures; or
- copied API shape, naming, examples, or scenario structure.

The existing agent-doc/editor/plugin/tmux/project-controller fixtures in
`lazily-spec` are not eligible as consumer-simulation examples. New examples use
neutral synthetic actors and values with no semantic mapping to a consumer.

## Prior-work and employer boundary

Before any consumer- or employer-related implementation or publication, obtain a
written record that:

- identifies the existing Lazily repositories and releases as prior work;
- reserves generic simulation APIs, traces, conformance fixtures, generators,
  shrinkers, and backend-neutral specifications to the existing Lazily project;
- assigns product-specific workflows, private schemas, deployment configuration,
  policies, and confidential business logic separately;
- states whether generic improvements may be contributed upstream;
- states the policy for employer time, equipment, credentials, and accounts; and
- routes patentable or genuinely novel implementation through the agreed
  disclosure process.

Record only the determination, date, and private location of the governing
document. Do not copy confidential agreement text into Lazily. If no consumer or
employer relationship applies, the owner must explicitly record “not applicable”
with a date before the consumer-adapter phase. Signed agreements and applicable
law control; this project gate is not legal advice.

## Backend-neutral simulation contract v1

This private contract is the implementation authority until an authorized
license decision permits a sanitized public form under `lazily-spec`. Normative
terms (`MUST`, `MUST NOT`, `SHOULD`) are intentional. Language APIs may use
idiomatic types, but their observable behavior and portable trace bytes must
conform to this section.

### Version identity and closed value algebra

Every world and trace header declares these identities independently:

- `contract_version = "lazily.sim/1.0"` for behavioral semantics;
- `trace_schema_version = 1` for the trace field layout;
- `canonical_encoding = "lazily-sim-json-v1"`;
- `rng_algorithm = "lazily-sim-rng-v1"`; and
- `digest_algorithm = "sha256/v1"`.

Readers MUST reject unsupported versions before starting a run. A schema version
changes for any field or encoding change. A contract version changes when clock,
ordering, stepping, port, fault, or invariant semantics change. Each extensible
`kind` also carries its own version; behavior MUST NOT change under a stable
kind/version pair. Recorded traces are immutable: an upgrade creates a new trace,
and an explicit migration records both the source and destination digests.

Portable values form a closed algebra:

- `null` and booleans;
- signed and unsigned 64-bit integers, carried as canonical decimal strings;
- valid Unicode strings in NFC form;
- byte strings, carried as lowercase hexadecimal;
- ordered lists of portable values; and
- maps with unique string keys.

Floats, host pointers, functions, channels, native objects, duplicate map keys,
invalid Unicode, and values outside the declared integer ranges are rejected.
An opaque domain value is an `EncodedValue { codec, type_tag, bytes_hex }` whose
codec and type tag are versioned. Simulation boundaries exchange encoded values,
not language objects or untyped JSON numbers.

Stable identifiers are nonempty ASCII strings matching
`[a-z][a-z0-9._:-]{0,127}`. IDs are unique within their namespace. References
must resolve before execution, and causal references may point only backward.

### Concept contracts

| Concept | Portable data contract | Runtime responsibility |
| --- | --- | --- |
| `LogicalTime` | Unsigned 64-bit tick, encoded as decimal text with `0` as the only zero spelling | Starts at the declared initial tick, never decreases, and is advanced only by `World` |
| `Seed` | `{ algorithm, algorithm_version, value_hex }`, where `value_hex` is exactly 32 bytes | Supplies named deterministic streams; ambient randomness is forbidden |
| `ActorSpec` | `{ id, kind, version, initial, config? }` using `EncodedValue` | Binds a stable actor identity to the production state/reducer adapter |
| `Action` | `{ id, actor_id, kind, version, payload, cause_id? }` | Represents one intended stimulus; each dequeued action is accepted or rejected exactly once |
| `Decision` | `{ accepted, next_state?, schedule[], port_calls[], observations[] }` | Is the ordered result of the same pure reducer/decision function used by production |
| `Observation` | `{ id, kind, version, actor_id?, payload }` | Reports a fact after settled execution and MUST NOT mutate the world |
| `PortSpec` | `{ id, kind, version, config? }` | Declares one nondeterministic external boundary with simulated and real adapters |
| `PortCall` | `{ id, port_id, operation, version, request }` | Carries an ordered effect intent outside the reducer |
| `PortResult` | `{ call_id, outcome, response? }`, where outcome is `ok`, `error`, `timeout`, or `cancelled` | Returns only as a scheduled action; synchronous actor re-entry is forbidden |
| `FaultSpec` | `{ id, kind, version, port_id, trigger, config }` | Targets only a declared port/operation; concrete faults are added by `#lzsimfaults` |
| `InvariantSpec` | `{ id, kind, version, config? }` | Binds a pure evaluator over a settled world view and trace prefix |
| `WorldSpec` | Versions, initial time, ordered actors, ports, and invariants | Defines the reusable subject topology without hidden process state |
| `RunLimits` | Maximum steps, logical-time ceiling, queue size, trace entries, and per-port pending calls | Is immutable scenario input; reaching a limit is visible and never means success |
| `Scenario` | Versioned `WorldSpec`, fully materialized initial actions, fault plan, seed identity, generator identity, and `RunLimits` | Is the immutable authoritative input for replay and shrinking |
| `Trace` | Versioned header, immutable ordered entries, terminal status, final world digest, and trace digest | Explains every state-affecting step and binds the run to exact inputs |
| `SimulationFingerprint` | Scenario digest, trace digest, subject/scheduler identities, and ordered checkpoint digests | Refuses incompatible comparisons before comparing any state |

An actor runtime has the conceptual operation
`decide(state, action) -> Decision`. It MUST call the production reducer or
decision function; a simulator-only reimplementation of business behavior is
non-conforming. `Decision.schedule`, `Decision.port_calls`, and
`Decision.observations` are semantically ordered lists. A separate reference
model may be an oracle, but it can never replace the production path or become
the sole oracle for the same behavior.

### Explicit port boundary

Actors and invariants cannot read wall time, use ambient randomness, sleep,
spawn uncontrolled goroutines/threads, access the network/filesystem/database,
or call a real service. An actor receives only its state plus an `Action` and
returns a `Decision`. All nondeterministic effects leave through a declared
`PortCall`.

A port receives `PortContext { now, trace_seq, action_id, attempt }` plus one
call and returns a terminal `PortResult` or a declared pending disposition. The
world schedules the result back into the queue; ports cannot invoke actors
synchronously. Simulated and real adapters consume and produce the same portable
shapes. Faults may delay, drop, duplicate, reorder, or replace results only at
this boundary, and every fault activation, decision, injection, recovery, and
result is trace-visible.

This makes the boundary explicit:

```text
versioned materialized Scenario
  -> deterministic World queue and logical clock
  -> the production reducer/decision function
  -> ordered Decisions and declared PortCalls
  -> reactive settlement
  -> portable observations and invariant results
  -> versioned canonical Trace
```

### World and actor lifecycle

`World` owns the logical clock, seed streams, actor registry, declared ports,
pending queue, invariant registry, and trace builder. Its minimal operations are:

- `Build(WorldSpec)` / `Reset(WorldSpec)` to create a fresh world;
- `Now()` to read logical time;
- `Submit(Action)` and `Schedule(at, Action)` to enqueue work;
- `Step()` to process exactly one scheduled item;
- `RunUntilIdle(max_steps)` and `RunSteps(max_steps)` with mandatory bounds;
- `Observe()` to return the settled portable world view; and
- `Trace()` to return the immutable trace generated so far.

Actor lifecycle is `registered -> running -> crashed -> running` or
`registered/running -> stopped`. A crash cannot invent domain behavior: the
actor adapter declares which encoded state is durable and which ephemeral state
is lost. Restart reconstructs the same production adapter from the declared
durable state. Lifecycle changes are scheduled and traced like other work.

The simulator is not an operating-system/thread scheduler, model checker, or
substitute for real database, broker, network, and deployment tests. It models
only the ordering and faults declared by the scenario at explicit ports.

### Scenario authority and run bounds

`Scenario` and `Trace` are distinct. A scenario is immutable input; a trace is
append-only execution output. A generated run MUST persist the fully materialized
ordered actions and fault plan before execution. Its seed, PRNG version, generator
kind/version, and generation coverage labels remain provenance, but replay and
shrinking consume the materialized scenario and never rerun the generator.

`scenario_digest` is SHA-256 over canonical scenario bytes and binds the exact
world spec, subject kind/version, scheduler policy/version, initial state,
ordered actions, fault plan, seed identity, generator identity, and run limits.
Changing any of these creates a different scenario even if the run happens to
reach the same final state. A stable seed does not promise the same generated
scenario across generator versions.

All limits are explicit inputs. Zero-time livelock, queue growth, logical-time
ceiling, pending-port exhaustion, and trace-size exhaustion terminate as
`bound_exhausted` with the responsible limit and current counters recorded. They
cannot be reported as `idle`, `pass`, or a generic timeout.

### Logical time and stable scheduling

The world uses one single-threaded priority queue. Each scheduled item is
`ScheduledItem { at, ordinal, action }` and the total order is exactly
`(at, ordinal)` ascending:

1. `ordinal` is a world-global unsigned 64-bit counter allocated once when an
   item is enqueued and never reused.
2. Initial actions are enqueued in their declared array order. Bulk input from
   any unordered container is sorted by canonical action bytes before enqueue.
3. Items emitted by a `Decision` are enqueued in the decision's list order.
4. A zero-delay item receives the current tick and a later ordinal, so it follows
   every same-tick item that was already enqueued.
5. Scheduling `at < now`, ordinal exhaustion, time overflow, duplicate IDs, or an
   unresolved reference is deterministically rejected and traced. The world
   MUST NOT silently clamp invalid time.
6. When no item or declared timer remains, the world is idle. A bounded runner
   returns `idle`, `bound_exhausted`, `invariant_failed`, or `error`; it never
   spins or silently treats exhaustion as success.

Stable queue ordering does not strengthen Lazily's reactive semantics. Independent
sibling-effect callback order remains non-contractual. Only the reducer's ordered
decision and the settled observations after the reactive batch are portable.

### Step protocol

One `Step` is atomic with respect to the world:

1. Dequeue the minimum `(at, ordinal)` item and advance the clock to `at`.
2. Record the dequeue and clock advance.
3. Invoke the target actor once through its production reducer adapter.
4. Record accepted/rejected outcome and ordered decision contents.
5. Apply the accepted state change and settle the reactive graph as one batch.
6. Submit ordered port calls and enqueue emitted actions/results in declared
   order; no callback may re-enter the actor during the step.
7. Sample settled observations in lexicographic observation-ID order.
8. Evaluate invariants in lexicographic invariant-ID order.
9. Append every resulting trace entry and final per-step world digest before
   selecting another item.

Rejected actions leave actor state unchanged but remain trace-visible. Accepted
equal-state transitions are still accepted; an underlying reactive store may
suppress invalidation. Observations therefore sample state after settlement,
not incidental effect callback order.

Safety invariants return `pass` or `fail` after every step. A bounded-liveness
invariant additionally declares its trigger, step/tick deadline, and satisfaction
predicate and may return `pending` until its bound. Reaching a bound with pending
obligations is `bound_exhausted`/inconclusive, never a passing run. Invariants are
pure: they cannot schedule work, call ports, consume randomness, or mutate state.

### Seed and random-stream contract

`lazily-sim-rng-v1` uses named counter streams so adding one generator does not
perturb unrelated streams:

1. The root seed is 32 bytes.
2. A stream key is `SHA-256("lazily.sim.rng.v1\0" || root_seed || labels)`, where
   each UTF-8 label is length-prefixed by an unsigned 32-bit big-endian length.
3. Block `n` is `SHA-256("lazily.sim.block.v1\0" || stream_key || n)`, with `n`
   encoded as unsigned 64-bit big-endian.
4. Bytes are consumed in block order. Bounded integers use rejection sampling,
   never modulo bias or host-language RNG helpers.

Stream labels minimally include phase (`generation`, `actor`, or `fault`), stable
actor/fault ID when applicable, and action/attempt ordinal. The trace records the
root seed identity, RNG version, generated actions, and fault decisions, so replay
does not depend on an unrecorded future generator implementation.

### Deterministic serialization

`lazily-sim-json-v1` is compact UTF-8 JSON over the closed value algebra:

- object keys are unique and sorted by raw UTF-8 byte order after NFC validation;
- arrays preserve declared order;
- no insignificant whitespace or trailing newline participates in canonical
  bytes;
- booleans and `null` use the JSON literals;
- all integers, times, ordinals, and sequence numbers use schema-tagged canonical
  decimal strings with no leading zero and no negative zero;
- bytes use schema-tagged lowercase hexadecimal strings;
- strings escape quote, reverse solidus, and control characters using the fixed
  JSON short escapes (`\b`, `\f`, `\n`, `\r`, `\t`) or lowercase `\u00xx` for
  the remaining controls; other characters are emitted as UTF-8; and
- unknown fields, missing required fields, invalid native values, and duplicate
  keys fail before execution.

The world-spec digest is SHA-256 over exact canonical `WorldSpec` bytes. The
trace digest is SHA-256 over the canonical header excluding `trace_digest` plus
all canonical entries and the terminal record. Pretty rendering is never hashed.
The same canonical scenario bytes and supported implementation-version set MUST
yield byte-identical trace bytes and the same digest.

### Trace and replay bridge

The trace header contains all version identities, subject and scheduler identity,
the scenario and world-spec digests, seed/generator provenance, fault-plan digest,
and declared limits. `TraceEntry` contains:

- contiguous `trace_seq` from zero and nondecreasing `now`;
- entry `kind` and version;
- scheduled `at` and `ordinal` when applicable;
- stable subject/action/call/fault IDs and backward-only cause ID;
- canonical event data;
- settled observation and invariant results when applicable; and
- the per-step world digest.

Entry kinds cover enqueue/dequeue, clock advance, action acceptance/rejection,
actor lifecycle, port call/result, fault activation/injection/recovery,
observation, invariant result, error, bound exhaustion, and terminal status.
Every external call reaches exactly one terminal result or remains explicitly
pending in the terminal record. Every state-affecting transition must be
explainable from the trace prefix.

Executable accepted actions project to the existing binding-local replay log in
trace execution order. A fresh-world replay applies the same production reducer,
samples the same settled observations after every action, binds its fingerprint
to the exact trace/log digest, and reports the first diverging trace sequence and
label. Binding-local replay encoders may fingerprint decoded values, but they are
not the portable trace format.

`SimulationFingerprint` first compares scenario, contract, subject, scheduler,
codec, RNG, and run-limit identities. A mismatch is reported as its own
incompatibility and stops before state comparison. Only compatible fingerprints
compare ordered checkpoint/world digests and localize the first trace divergence.
Human previews and diagnostic strings are excluded from semantic digests and
must never influence replay.

### Go integration anchors

- `ManualClock` owns monotone time, but `SimWorld` validates and rejects backward
  requests before calling `Advance`, whose existing behavior clamps them.
- `Transition` and `StateMachine.Send` are the production pure-reducer path. The
  actor adapter receives that function once; it never duplicates transition
  logic in a test model.
- One decision applies inside `Context.Batch`; observations sample settled
  `StateMachine.State`/cells afterward. `OnTransition` and sibling effect order
  are not trace sources because batch settlement may coalesce or reorder them.
- Accepted executable inputs project to `ReplayEvent` with trace execution
  sequence preserved. `ReplayHarness` rebuilds a fresh graph and supplies the
  binding-local graph replay proof, but it does not prove the scenario, scheduler,
  seed, fault plan, subject version, or run limits. `SimulationFingerprint` owns
  those wider obligations.
- `ReplayCanonicalBytes` remains binding-local, as its own contract states. The
  new `lazily-sim-json-v1` serializer owns cross-binding trace bytes.
- Add a distinct `Simulation` binding capability only when the public contract
  and implementation land; do not infer simulation support from `StateMachine`.

### Contract acceptance invariants

An implementation of this v1 contract must prove all of the following:

1. All IDs are unique in their namespace, every reference resolves, every causal
   reference points backward, and every declared version is supported before run.
2. Clock values never decrease; past scheduling and overflow fail visibly; queue
   keys are unique and dequeue order is monotonically `(at, ordinal)`.
3. Same canonical scenario and supported implementation versions produce
   byte-identical traces and equal digests across repeated same-process and
   cold-process runs; the materialized scenario, not regeneration from a seed,
   is authoritative.
4. No wall clock, sleep, ambient randomness, uncontrolled concurrency, or
   undeclared I/O occurs inside world, actor, port simulation, or invariant code.
5. Every external interaction is a declared port call, with exactly one terminal
   result or an explicit pending disposition at bounded termination.
6. Each dequeued action has exactly one accepted/rejected result; accepted actions
   invoke the production reducer/decision path, never simulator-only business code.
7. Observations and invariant results are sampled after every settled action and
   are trace-visible; invariant code is side-effect-free.
8. Trace sequence is contiguous, time is nondecreasing, canonical encoding rejects
   unsupported values, and map iteration cannot affect scheduling or bytes.
9. Fresh-world replay yields the same per-step observations and world digests and
   localizes the first mismatch by trace sequence and label.
10. Stable world-queue order does not assert an ordering for independent reactive
    sibling effects; only settled portable observations are contractual.
11. Scenario, contract, subject, scheduler, codec, RNG, fault-plan, and run-limit
    mismatches refuse comparison before any state or checkpoint comparison.
12. Omitting, duplicating, or reordering a semantic trace entry changes the trace
    digest; diagnostic preview changes do not.

## Phase-admission checklist

- [x] FoundationDB, QuickCheck, Go fuzzing, and Herlihy-Wing primary/official
  sources are recorded and classified as reference-only.
- [x] Current Go and spec-tooling dependencies are inventoried with versions and
  licenses.
- [x] The existing canonical and vendored fixture sets are counted and their
  current provenance metadata is documented.
- [x] Independent-authorship and excluded-material rules are recorded.
- [x] The private backend-neutral v1 contract defines versioned concepts, explicit
  ports, scenario authority, logical time, stable ordering, canonical bytes,
  trace/fingerprint semantics, production-reducer reuse, and acceptance invariants.
- [x] The prior-work/employer determination required before consumer-related work
  is defined.
- [ ] An authorized owner has chosen and recorded the `lazily-spec` repository
  license and fixture reuse terms (`#lzsimlicense`).
- [ ] The repository metadata, third-party inventory, and fixture provenance
  chain match that decision (`#lzsimlicense`).
- [ ] Before each implementation phase, its reviewer and review date are added
  here and every applicable open condition is closed.

## Verification evidence

The gate was built from repository manifests, lock files, license files, fixture
counts, generator metadata, git authorship history, official repository license
endpoints, and the primary/official sources linked above. It intentionally adds
no runtime dependency and imports no third-party code or fixture bytes.
