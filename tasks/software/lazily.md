---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 24f78fda-2db5-41b6-8eb1-52772d913df8
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: go
---

## Status

<!-- agent:status patch=replace -->

active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260727-202047.md`*

Compacted content:
- Archived 16 response topic(s): review triage; queue materialization repair; controller recycle boundary; 13 more
- Prior summary/context: prior compacted content: Archived 14 response topic(s): Response; Response; Response; 11 more
Implementation is complete, green, committed, pushed, and CI passed. IDEA still runs plugin 0.2.299; plugin 0.2.300 is installed but requires activation.

Please restart IDEA once more, then send `continue` so I can commit the agent-doc closeout.

### Re: Response

Implementation is complete, green, committed, pushed, and CI passed. IDEA still runs plugin 0.2.299; plugin 0.2.300 is installed but requires activation.

### Re: do [#lzcstemporal] — gpt-5

> **Queue prompt:**
>
> do [#lzcstemporal]


Completed and shipped the full lazily-cs temporal, rate-shaping, and windowing phase.

- Added `Optional<T>` plus production logical-clock `ManualLogicalClock`, `TimerCell`, `IntervalCell`, `CronCell`, and value-preserving `DeadlineCell<T>`.
- Added `DebounceCell<T>`, leading/trailing `ThrottleCell<T>`, count/time `SampleCell<T>`, deterministic `SplitMix64Random`, and strict-threshold `ProbabilisticSampleCell<T>`.
- Added count/time tumbling, count sliding, and gap-based session windows over the production `MergePolicy<T>` algebra.
- Every emitted output is a real reactive `Source`; held, dropped, early, and empty-window operations do not touch it, so fixture invalidation assertions exercise the library rather than runner bookkeeping.
- Added `TemporalRateWindowConformanceTests.cs`, which discovers and opens all 4 temporal, 6 rate-shaping, and 4 windowing canonical fixtures, asserts exact returns/state/output/invalidation behavior, and carries positive fixture and step floors.
- Removed exactly those 14 runtime-opened fixtures from the known-uncovered ledger. Coverage advanced from 61/124 with 63 known-uncovered to 75/124 with 49 known-uncovered.
- Mutation proof: changing probabilistic admission from `draw < rate` to `draw <= rate` made the canonical rate-shaping runner fail at the exact `draw == 0.5` boundary; the restored implementation passed.

Verification and publication:

- Targeted phase suite: 5/5 passed.
- lazily-cs `make check`: 136/136 passed; coverage gate reported 75/124 opened and 49 known-uncovered.
- lazily-cs `make format-check`: passed.
- Workspace `make check`: passed, including 8,222/8,222 agent-doc tests and the remaining workspace suites.
- lazily-cs CI for `d5bba73`: completed successfully on the required one-shot inspection.
- Root repository has no workflow named `CI`.
- Pushed lazily-cs commit `d5bba73`; pushed root submodule pointer `6434c3b53`.
- Preserved pre-existing lazily-cs `AGENTS.md`, `README.md`, and untracked `build/`, plus unrelated workspace changes.

Confidence is high: the complete canonical corpora, true reactive invalidation probes, merge-policy-path counter, large logical-clock jump, mutation check, full binding gate, workspace gate, and CI are green. No escalation was required.

C# may now join the exact temporal, rate-shaping, and windowing peer groups in the single feature suite. Unsupported capabilities remain staged and skipped rather than advertised.
<!-- agent:boundary:0f4b5f8a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lzcstemporal]~~
- do [#lzcscluster]
- do [#lzcspackage]
- do [#asyncplanenever]
- do [#lazilyconformancecoverag]
- /goal Add missing lazily-cs features
- release + publish all lazily packages
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzcsqueues] lazily-cs: PARTIAL/STAGED — QueueCell + QueueStorage shipped at 01d2cf3; WorkQueueCell shipped at fafb88f; TopicCell shipped at 17256ce; Relay core and Count BackpressurePolicy shipped at 5ac24cc; SpillStore shipped at 3e79b92; Relay Transport shipped at 0cca2e6; Spill overflow integration shipped at c5d678d with idempotent-or-deduplicated replay guard and lossless cold+hot reconstruction. Keep C# eligible only for exact capabilities. Still add Bytes/Keys/Age meters before admitting C# to the full RelayCell peer group. (stays open: Bytes/Keys/Age metering and normative integration tests remain.)
- [ ] [#lzcscrdt] lazily-cs: PARTIAL/STAGED — HLC, ICellCrdt/IRegisterCrdt, LWW/MV registers, PnCounter, and the ReplicatedCell reactive bridge shipped at f1f32e2 with algebraic, replay-idempotence, and invalidation-suppression tests. Keep C# eligible only for these exact CRDT register capabilities. Still implement TextCrdt + delta sync, SeqCrdt, CrdtTree, and LosslessTreeCrdt including dotted-frontier anti-entropy and concurrent-merge convergence, then replay the textcrdt, seqcrdt, crdt-tree, and lossless-tree corpora before full CRDT peer admission. Plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24). (stays open: TextCrdt/delta, SeqCrdt, CrdtTree, LosslessTreeCrdt, and their conformance runners remain.)
- [ ] [#lzcsipc] lazily-cs: PARTIAL/STAGED — schema-defined Snapshot/Delta/CrdtSync wire types, exact externally tagged codec, all 10 DeltaOp variants, WireStamp/frontier/CRDT frames, blob descriptors, and Draft 2020-12 validation shipped at 13e8d67. Runtime conformance coverage is now 56/124 with 68 known-uncovered. Admit C# only to Snapshot/Delta/CrdtSync schema and wire peer groups. Still implement ShmBlobArena and executable BlobBackend zero-copy resolution/transport for shm, Arrow, and in-process backends before rows 26-27 admission. Plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27). (stays open: shared-memory arena and zero-copy backend behavior/tests remain.)
- [ ] [#lzcssync] lazily-cs: PARTIAL/STAGED — storage-independent IOutboxStore/DurableOutbox protocol, InMemoryOutboxStore, fsync append-only FileOutboxStore, monotonic stale-handle cursor recovery, append-before-send crash replay, and canonical outbox_store_protocol/outbox_replay_after_crash replay shipped at b103f80. Runtime coverage is now 58/124 with 66 known-uncovered. Admit C# only to the durable-outbox peer group. Still implement CrdtPlaneRuntime + anti-entropy, resync coordinator, IpcSink/IpcSource, and full-duplex SyncDriver; replay remaining reliable-sync, distributed, and message-passing corpora. Plan rows 28-32. (stays open: CRDT plane runtime, anti-entropy/resync, transports, sync driver, remaining corpora.)
- [ ] [#lzcsplane] lazily-cs: PARTIAL/STAGED — CausalReceipt/CausalReceipts schema/projection shipped at 967c44e; all 17 typed signaling frames, strict exact codec, authoritative SignalingRoom, anti-spoof server-stamped sender, sorted self-excluding rosters, unknown-target handling, and leave semantics shipped at d663074 with canonical signaling client/server fixture replay. Runtime coverage is now 61/124 with 63 known-uncovered. Admit C# only to the exact causal-receipts and signaling-wire/open-room peer groups. Still implement general state projection/mirror, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay service, agent-doc, and remaining message-passing corpora. Plan rows 33-39. (stays open: state mirror, command plane, FFI, permissions, handshake, instrumentation/benchmarks, and remaining corpora.)
- [ ] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [ ] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
- [ ] [#asyncplanenever] STAGED implementation — finish the async v2 kernel naming migration in C++, Go, Python, and Zig per tasks/plan-lazily-async-v2-migration.md. Rust, JavaScript, Kotlin, and C# already expose AsyncSource/AsyncComputed. Preserve deprecated aliases in published bindings, collapse live memo constructors into guarded computed, and settle the public AsyncComputedState vs formal AsyncSlotState distinction without renaming the formal model mechanically.
- [ ] [#lazilyconformancecoverag] lazily-cs: STAGED per operator — incomplete C# surfaces stay out of peer/feature test groups until implemented. Fresh runtime measurement 2026-07-27: 75/124 canonical fixtures opened, 49 known-uncovered. Temporal (4), rate-shaping (6), and windowing (4) are now runtime-opened at d5bba73 alongside the previously admitted reactive graph, collections, statechart, materialization, queue, IPC, durable-outbox, causal-receipts, and signaling slices. Keep open and work the remaining ledger down only by shipping explicit tasks in tasks/software/lazily-cs-parity-plan.md; admit C# to each capability group only when that production surface and canonical replay ship.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzinteropcs] Keep lazily-cs visible but registry-staged in the single network suite. Current state: #lzcsipc is partial (schema-validated wire codec shipped; ShmBlobArena/executable zero-copy backends remain) and #lzcssync is partial (durable outbox shipped; CrdtPlaneRuntime, anti-entropy/resync, IpcSink/IpcSource, full-duplex SyncDriver, and remaining corpora remain). After both prerequisites ship their complete production surfaces, implement the C# interop peer, advertise only the exact features/codecs/channels/variants it supports, and let the capability-driven matrix admit it automatically. (gated: prerequisites #lzcsipc and #lzcssync remain open.)
- [/] [#lazilyrenameinverted] [operator-decision] Keep lazily-py `Cell`/`CellSlot` as mypyc-native implementation class names with canonical `Source`/`SourceSlot` aliases. NEXT: accept this documented exception, or explicitly authorize a breaking native-class rename with mypyc and identity migration tests.
- [/] [#lzjsnpmtrustedpub] [operator-action] lazily-js repository workflow/OIDC/provenance is ready, but npm rejects publication until trusted-publisher setup is completed. NEXT: authenticate npm, register `@lazily-hub/lazily-js` for `lazily-hub/lazily-js` workflow `release.yml` with no environment and allowed action `npm publish`, verify with `npm trust list @lazily-hub/lazily-js`, then prove the next version tag.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
