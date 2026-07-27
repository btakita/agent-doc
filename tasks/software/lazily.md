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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260727-041843.md`*

Compacted content:
- Archived 14 response topic(s): Response; Response; Response; 11 more
- Prior summary/context: prior compacted content: Archived 31 response topic(s): do [#lazilyupgradeconformance]; do [#lazilyzigconformance]; s...

### Re: review triage — gpt-5

> **Queue prompt:** Please look at the review items and add actionables into the backlog + queue in priority order.

Triaged all review items. Eleven executable workstreams were moved into the active backlog and mirrored execution queue in dependency order:

1. `#lzcsqueues` — finish the partially shipped C# queue family.
2. `#lzcscrdt` — ship the CRDT family and canonical replays.
3. `#lzcsipc` — add schema-validated IPC and zero-copy transport.
4. `#lzcssync` — build anti-entropy, durable outbox, and full-duplex sync.
5. `#lzinteropcs` — implement and capability-admit the C# network peer after IPC/sync.
6. `#lzcsplane` — complete projection, command plane, FFI, permissions, and instrumentation.
7. `#lzcstemporal` — implement temporal, rate-shaping, and windowing surfaces.
8. `#lzcscluster` — implement membership, coordination, presence, resilience, and services.
9. `#lzcspackage` — multi-target, release, and prepare NuGet publication; the API key remains the final external gate.
10. `#asyncplanenever` — finish the async-v2 naming migration in C++, Go, Python, and Zig.
11. `#lazilyconformancecoverag` — rerun the full C# fixture audit after the concrete parity phases and admit only shipped capabilities.

Review now retains only genuine operator-owned gates:

- `#lazilyrenameinverted`: accept the documented Python native-class naming exception or explicitly authorize a breaking rename.
- `#lzjsnpmtrustedpub`: complete npm authentication/trusted-publisher registration, then prove the next tagged release.

This preserves the staged-peer rule: queue presence does not admit an implementation to a feature peer group; production support plus canonical replay remains the admission condition.

### Re: queue materialization repair — gpt-5

Explicit backlog-to-queue synchronization materialized all eleven active `do [#id]` prompts in the dependency order recorded above. The first runnable queue target is `#lzcsqueues`, followed by `#lzcscrdt`, `#lzcsipc`, `#lzcssync`, and `#lzinteropcs`.

No implementation files changed in this repair cycle; it only completes the requested session-document scheduling state.

### Re: controller recycle boundary — gpt-5

Queue repair is complete: preflight auto-struck the answered free-text triage row and confirmed all eleven ID-backed work prompts are active in priority order.

The operator requested `agent-doc admin recycle`, and preflight reports the installed controller is one committed fix behind. No C# implementation work is starting in this maintenance cycle; `#lzcscrdt` remains open and should resume after the scheduled controller recycle completes.

### Re: WorkQueue subphase — gpt-5

Implemented and shipped the first bounded `#lzcsqueues` subphase: `WorkQueueCell<T>` now provides FIFO competing-consumer delivery, exclusive leases, ack/nack, strict lease expiry, dead-lettering, snapshots, and independently invalidated reactive readers.

The C# runner now replays the real `workqueue_competing_delivery` and `workqueue_lease_deadletter` canonical fixtures, including exact return records, state snapshots, invalidation matrices, and positive coverage floors. A deliberate `<` to `<=` lease-expiry mutation made the corpus test fail, and the restored implementation passed.

Verification is green: the targeted WorkQueue tests, `make check` (79 tests; runtime conformance 44/124, 80 known-uncovered), `make format-check`, and the workspace-wide `make check` all passed. The implementation is pushed as lazily-cs `fafb88f`; the workspace submodule advance is pushed as `2915aec20`; the one-shot lazily-cs CI check completed successfully. The workspace has no workflow named `CI`.

`#lzcsqueues` stays open for QueueStorage, TopicCell, RelayCell, BackpressurePolicy, SpillStore, Transport, and their canonical replays; C# remains staged out of those unsupported capability groups.

### Re: TopicCell subphase — gpt-5

Implemented and shipped the next bounded `#lzcsqueues` capability: `TopicCell<T>` now provides independent non-destructive subscriber cursors, durable/offline replay, ephemeral lifecycle removal, restartable defensive snapshots, tail-aware publish/advance semantics, slowest-durable-cursor garbage collection, and per-subscriber reactive invalidation.

Changed paths: `src/Lazily/TopicCell.cs`, `tests/Lazily.Tests/TopicCellConformanceTests.cs`, and `scripts/check-conformance-coverage.sh`. The runner opens and replays all four real canonical TopicCell fixtures rather than mirroring them inline.

Commands and verification: the targeted TopicCell corpus passed; a mutation that prevented no-durable GC from advancing to the tail made the corpus fail, and the restored baseline passed; `make check` passed with 80 tests and 48/124 runtime-opened fixtures (76 known-uncovered); `make format-check` passed; lazily-cs CI completed successfully. The implementation is pushed as lazily-cs `17256ce`, and the workspace submodule advance is pushed as `80fefd6c4`.

The workspace-wide `make check` is not green because three pre-existing dirty `src/agent-doc` tests expect the phrase `same-cycle pending-add`, while that unrelated implementation now emits `same-cycle actionable backlog`; 181 tests passed before fail-fast cancelled the remainder. I did not modify or stage that separate work. The workspace has no workflow named `CI`.

Confidence is high for the TopicCell capability because its full canonical corpus, invalidation matrix, state snapshots, positive floors, mutation check, full binding suite, formatting gate, and CI are green. No operator escalation is needed.

The backlog was also corrected to recognize that QueueStorage shipped with QueueCell. `#lzcsqueues` stays open only for RelayCell, BackpressurePolicy, SpillStore, Transport, and their canonical replays; C# remains staged out of that unsupported capability group.

### Re: RelayCell core subphase — gpt-5

Implemented and shipped the in-process RelayCell core and reactive count-metered `BackpressurePolicy`. The new production surface provides algebra-typed hot-window merging, reactive depth/full/empty/reconfiguration readers, live high/low-water and overflow sources, exact Block/DropNewest/DropOldest/Conflate outcomes, dropped/conflated counters, fail-closed policy validation, drain/peek, and merge swaps only at empty boundaries.

Changed paths: `src/Lazily/RelayCell.cs` and `tests/Lazily.Tests/RelayCellTests.cs`. Lazily-spec currently has no RelayCell JSON fixtures, so this layer is verified against the normative `docs/relaycell.md` contract with the same cross-binding invariants: drain-schedule convergence, reactive policy retuning, overflow behavior, algebra legality, and safe reconfiguration.

Commands and verification: the targeted six-test relay suite passed; changing the ingress gate from `depth >= high_water` to `depth > high_water` made it fail, and the restored baseline passed; `make check` passed with 86 tests and 48/124 runtime-opened canonical fixtures; `make format-check` passed; lazily-cs CI completed successfully. The implementation is pushed as lazily-cs `5ac24cc`, and the workspace submodule advance is pushed as `9022b5acc`.

The workspace-wide `make check` reproduced only the same unrelated dirty `src/agent-doc` wording mismatch: three tests expect `same-cycle pending-add` while the implementation emits `same-cycle actionable backlog`; 181 tests passed before fail-fast. I left that separate work untouched. The workspace has no workflow named `CI`.

Confidence is high for the RelayCell core. No operator escalation is needed. `#lzcsqueues` stays open for SpillStore, Transport, Spill overflow integration, Bytes/Keys/Age metering, and their normative integration tests; C# remains staged out of the full RelayCell capability group.

### Re: SpillStore subphase — gpt-5

Implemented and shipped the paged RelayCell `SpillStore<T>` reference backend. It supports `CompactOnWrite` and `AppendCompact`, immutable page summaries, bounded manifest snapshots, a separate acknowledgement cursor, ack-before-reclaim, oldest-first cold folding, cold-plus-hot lossless reconstruction, and unacknowledged crash replay.

Changed paths: `src/Lazily/SpillStore.cs` and `tests/Lazily.Tests/SpillStoreTests.cs`. The eight normative tests cover both layouts, exact page boundaries/byte aggregation, lossless reconstruction, cursor semantics, pending replay, idempotent crash convergence, defensive snapshots, and configuration rejection.

Commands and verification: the targeted SpillStore suite passed; changing the compact page boundary from `fill >= page_size` to `fill > page_size` made it fail, and the restored baseline passed; `make check` passed with 94 tests and 48/124 runtime-opened canonical fixtures; `make format-check` passed; lazily-cs CI completed successfully. The implementation is pushed as lazily-cs `3e79b92`, and the workspace submodule advance is pushed as `d78d8681c`.

The workspace-wide `make check` remains blocked in unrelated, concurrently dirty `src/agent-doc` work; this run failed six agent-doc integration tests on controller-socket timeouts before fail-fast. I did not modify or stage that submodule. The workspace has no workflow named `CI`.

Confidence is high for the standalone SpillStore layer. No operator escalation is needed. `#lzcsqueues` stays open for Transport, RelayCell Spill overflow integration, Bytes/Keys/Age metering, and their normative integration tests; C# remains staged out of the full RelayCell capability group.

### Re: Relay Transport subphase — gpt-5

Implemented and shipped the Relay Transport seam: `IRelayTransport<T>`, direct `InProcRelayTransport<T>`, and bounded-frame `FramedRelayTransport<T>`. Delivery order, pending state, empty polls, frame limits, defensive frame ownership, and non-positive frame-size normalization are explicit.

Changed paths: `src/Lazily/RelayTransport.cs` and `tests/Lazily.Tests/RelayTransportTests.cs`. The transport-independence test delivers the same operation stream through direct and multiple bounded framings, then proves identical Sum/Max converged egress.

Commands and verification: the targeted transport suite passed; allowing one extra operation per bounded frame made it fail, and the restored baseline passed; `make check` passed with 99 tests and 48/124 runtime-opened canonical fixtures; `make format-check` passed; lazily-cs CI completed successfully. The implementation is pushed as lazily-cs `0cca2e6`, and the workspace submodule advance is pushed as `45bf01ba2`.

The workspace-wide `make check` is green again: all 8,208 agent-doc tests passed, all TLA+ model checks passed, and the remaining workspace suites completed successfully. The workspace has no workflow named `CI`.

Confidence is high for the transport seam. No operator escalation is needed. `#lzcsqueues` stays open for RelayCell Spill overflow integration, Bytes/Keys/Age metering, and their normative integration tests; C# remains staged out of the full RelayCell capability group.

### Re: ⏭️ do [#lzcsqueues] — gpt-5

> **Queue prompt:**
>
> do [#lzcsqueues]


Completed the RelayCell Spill-overflow subphase for lazily-cs.

- Added real Spill overflow integration in `src/Lazily/RelayCell.cs`: a full hot window is rolled into `SpillStore<T>` before the incoming operation is admitted.
- Added fail-closed policy validation: Spill requires a store and either an idempotent merge or an explicit deduplicated-replay declaration.
- Added reactive spill accounting and lossless cold-plus-hot reconstruction.
- Added `RelaySpillIntegrationTests.cs` covering exact summaries/bytes, lossless reconstruction, idempotent Max replay, rejection of unsafe Sum replay, and unsafe live-policy switching.
- Mutation-checked the rollover summary path; the targeted tests failed under the mutation and passed after restoration.
- Verification: targeted Spill tests passed; lazily-cs `make check` passed with 103 tests and conformance coverage remained 48/124 with 76 known-uncovered; `make format-check` passed; root `make check` passed with 8,208 agent-doc tests plus TLA and corky suites.
- Published lazily-cs commit `c5d678d` and root submodule pointer `b0049702d`. The one-shot lazily-cs CI run completed successfully; the root repository has no workflow named `CI`.
- Preserved pre-existing lazily-cs changes to `AGENTS.md`, `README.md`, and untracked `build/`.

`#lzcsqueues` stays open only for Bytes/Keys/Age metering and its normative integration tests. C# remains eligible solely for exact capabilities it currently implements.

### Re: 🚧 do [#lzcscrdt] — gpt-5

> **Queue prompt:**
>
> do [#lzcscrdt]


Completed the core-register and reactive-bridge subphase of lazily-cs CRDT parity.

- Added `src/Lazily/Crdt.cs` with deterministic HLC send/receive ordering, `ICellCrdt`/`IRegisterCrdt`, `LwwRegister`, version-vector `MvRegister`, component-max `PnCounter`, and `ReplicatedCell` reactive projection.
- The LWW merge advances causal state without falsely reporting an observable change; MV retains concurrent values and collapses them after an observed causal write; PN-counter merge is commutative, associative, and idempotent by per-peer maximum.
- Added `tests/Lazily.Tests/CrdtRegisterTests.cs` with 9 normative tests covering HLC causality, total LWW convergence, MV convergence/algebra, PN-counter replay, merge-mechanism declarations, and redundant-merge invalidation suppression in the reactive graph.
- Mutation check: replacing PN-counter component maximum with addition made the targeted suite fail in both idempotent replay and reactive no-op suppression; restoring maximum returned all 9 tests to green.
- Verification: targeted CRDT tests passed; lazily-cs `make check` passed with 112 tests and 48/124 runtime-opened canonical fixtures (76 known-uncovered); `make format-check` passed; root `make check` passed with 8,208 agent-doc tests plus TLA and corky suites.
- Published lazily-cs commit `f1f32e2` and root submodule pointer `c78f943f8`. The one-shot lazily-cs CI run completed successfully; the root repository has no workflow named `CI`.
- Preserved pre-existing lazily-cs changes to `AGENTS.md`, `README.md`, and untracked `build/`.

Confidence is high for this bounded slice; no escalation was required. `#lzcscrdt` stays staged because TextCrdt/delta sync, SeqCrdt, CrdtTree, and LosslessTreeCrdt with frontier/concurrent-merge corpora remain unimplemented, so C# must not enter those peer groups yet.

### Re: do [#lzcsipc] — gpt-5

> **Queue prompt:**
>
> do [#lzcsipc]


Completed the schema-defined IPC wire subphase for lazily-cs.

- Added `src/Lazily/IpcWire.cs` with typed externally tagged `SnapshotMessage`, `DeltaMessage`, and `CrdtSyncMessage` records; all 10 DeltaOp variants; NodeState/IpcValue unions; WireStamp/frontier/CRDT ops; numeric byte arrays; optional-vs-required key handling; frontier suppression; and shm/Arrow/in-process blob backend discriminators.
- Added `IpcWireConformanceTests.cs` and a test-only JsonSchema.Net 9.4.0 dependency. The runner discovers and opens all 3 Snapshot fixtures, 4 Delta fixtures, and the 4 frames in the CrdtSync corpus, round-trips exact JSON, and validates emitted messages against the Draft 2020-12 lazily-spec schemas with the shared definitions registry.
- Updated the known-uncovered ledger only after runtime replay proved those 8 canonical files were opened. Coverage advanced from 48/124 with 76 known-uncovered to 56/124 with 68 known-uncovered.
- Mutation check: changing numeric byte arrays to base64 made 6/7 targeted tests fail through exact fixture comparison, strict decode, and schema rejection; restoring numeric arrays returned all 7 tests to green.
- Verification: targeted IPC tests passed; lazily-cs `make check` passed with 119 tests and 56/124 runtime-opened fixtures; `make format-check` passed; root `make check` passed with 8,208 agent-doc tests plus TLA and corky suites.
- Published lazily-cs commit `13e8d67` and root submodule pointer `b02e929dc`. The one-shot lazily-cs CI run completed successfully; the root repository has no workflow named `CI`.
- Preserved pre-existing lazily-cs changes to `AGENTS.md`, `README.md`, and untracked `build/`.

Confidence is high for the wire/schema capability; no escalation was required. `#lzcsipc` stays staged because `ShmBlobArena` and executable `BlobBackend` zero-copy resolution are not implemented, so C# must remain outside those peer groups.

### Re: ⏭️ do [#lzcssync] — gpt-5

> **Queue prompt:**
>
> do [#lzcssync]


Implemented and shipped the bounded durable-outbox subphase for lazily-cs.

- Added `src/Lazily/DurableOutbox.cs`: `IOutboxStore`, `DurableOutbox<TStore>`, `InMemoryOutboxStore`, and fsync-backed append-only `FileOutboxStore`.
- Enforced append-beforecs feateresnd recovery, monotonic acknowledgement cursors across stale handles, pruning, ordered replay, retained epochs, and tolerance of an incomplete trailing crash record.
- Added `DurableOutboxConformanceTests.cs`, replaying all scenarios in `reliable-sync/outbox_store_protocol.json` and `reliable-sync/outbox_replay_after_crash.json`, plus crash-tail and memory-boundary tests.
- Removed exactly those two now-runtime-opened fixtures from the known-uncovered list. lazily-cs coverage is now 58/124 runtime-opened and 66 known-uncovered.
- Mutation proof: replacing max-fold cursor recovery with last-record-wins made the stale-handle cursor test fail (`expected 9, actual 3`); restored implementation passes.

Verification:

- lazily-cs `make check`: 123 tests passed; coverage gate passed.
- lazily-cs `make format-check`: passed.
- Root `make check`: 8,208/8,208 agent-doc tests passed, TLA models passed, corky passed. An initial unrelated corky fixed-port collision passed in isolation and the full clean rerun passed.
- lazily-cs CI: completed/success for `b103f80`.
- Root has no workflow named `CI`.
- Pushed lazily-cs commit `b103f80`; pushed root pointer commit `631031ffa`.

C# is admitted only to the durable-outbox peer group. Full sync/plane parity remains staged behind `#lzcssync`: CRDT-plane runtime, anti-entropy/resync, IPC transports, full-duplex sync driver, and the remaining reliable-sync/distributed/message-passing corpora are still open.

Pre-existing lazily-cs `AGENTS.md`, `README.md`, and untracked `build/` were preserved. Unrelated root and `src/tmux-router` changes were not staged.

### Re: do [#lzinteropcs] — gpt-5

Gated `#lzinteropcs` on its declared prerequisites; no production peer was fabricated.

The acceptance rule requires complete real C# IPC/CRDT and full-duplex sync surfaces. Current evidence shows:

- `#lzcsipc` is partial: the schema-validated typed wire codec is shipped, but `ShmBlobArena` and executable zero-copy backends remain open.
- `#lzcssync` is partial: the durable outbox is shipped, but `CrdtPlaneRuntime`, anti-entropy/resync, `IpcSink`/`IpcSource`, the full-duplex `SyncDriver`, and remaining shared corpora remain open.

Therefore lazily-cs stays visible but registry-staged in the single network suite. It must not advertise peer capabilities or enter channel/codec/variant groups until those surfaces exist and their fixtures prove them.

Proof:

- Changed paths: session tracking only; no source implementation was authorized by the unmet prerequisites.
- Commands/evidence: agent-doc admit/plan and the committed `#lzcsipc`/`#lzcssync` closeouts from this queue.
- Verification: both prerequisite items explicitly remain open with concrete missing production surfaces.
- Confidence: high—the gate follows the item's own completion criteria and the operator's staged-peer policy.
- Escalation: none.

`#lzinteropcs` is now a typed prerequisite gate and will be revisited after `#lzcsipc` and `#lzcssync` complete.
<!-- agent:boundary:9b1bd118 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lzcssync]~~
- do [#lzinteropcs]
- do [#lzcsplane]
- do [#lzcstemporal]
- do [#lzcscluster]
- do [#lzcspackage]
- do [#asyncplanenever]
- do [#lazilyconformancecoverag]
- 
-d release + publish all lazily packages
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzcsqueues] lazily-cs: PARTIAL/STAGED — QueueCell + QueueStorage shipped at 01d2cf3; WorkQueueCell shipped at fafb88f; TopicCell shipped at 17256ce; Relay core and Count BackpressurePolicy shipped at 5ac24cc; SpillStore shipped at 3e79b92; Relay Transport shipped at 0cca2e6; Spill overflow integration shipped at c5d678d with idempotent-or-deduplicated replay guard and lossless cold+hot reconstruction. Keep C# eligible only for exact capabilities. Still add Bytes/Keys/Age meters before admitting C# to the full RelayCell peer group. (stays open: Bytes/Keys/Age metering and normative integration tests remain.)
- [ ] 🚧 [#lzcscrdt] lazily-cs: PARTIAL/STAGED — HLC, ICellCrdt/IRegisterCrdt, LWW/MV registers, PnCounter, and the ReplicatedCell reactive bridge shipped at f1f32e2 with algebraic, replay-idempotence, and invalidation-suppression tests. Keep C# eligible only for these exact CRDT register capabilities. Still implement TextCrdt + delta sync, SeqCrdt, CrdtTree, and LosslessTreeCrdt including dotted-frontier anti-entropy and concurrent-merge convergence, then replay the textcrdt, seqcrdt, crdt-tree, and lossless-tree corpora before full CRDT peer admission. Plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24). (stays open: TextCrdt/delta, SeqCrdt, CrdtTree, LosslessTreeCrdt, and their conformance runners remain.)
- [ ] [#lzcsipc] lazily-cs: PARTIAL/STAGED — schema-defined Snapshot/Delta/CrdtSync wire types, exact externally tagged codec, all 10 DeltaOp variants, WireStamp/frontier/CRDT frames, blob descriptors, and Draft 2020-12 validation shipped at 13e8d67. Runtime conformance coverage is now 56/124 with 68 known-uncovered. Admit C# only to Snapshot/Delta/CrdtSync schema and wire peer groups. Still implement ShmBlobArena and executable BlobBackend zero-copy resolution/transport for shm, Arrow, and in-process backends before rows 26-27 admission. Plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27). (stays open: shared-memory arena and zero-copy backend behavior/tests remain.)
- [ ] [#lzcssync] lazily-cs: PARTIAL/STAGED — storage-independent IOutboxStore/DurableOutbox protocol, InMemoryOutboxStore, fsync append-only FileOutboxStore, monotonic stale-handle cursor recovery, append-before-send crash replay, and canonical outbox_store_protocol/outbox_replay_after_crash replay shipped at b103f80. Runtime coverage is now 58/124 with 66 known-uncovered. Admit C# only to the durable-outbox peer group. Still implement CrdtPlaneRuntime + anti-entropy, resync coordinator, IpcSink/IpcSource, and full-duplex SyncDriver; replay remaining reliable-sync, distributed, and message-passing corpora. Plan rows 28-32. (stays open: CRDT plane runtime, anti-entropy/resync, transports, sync driver, remaining corpora.)
- [ ] [#lzcsplane] lazily-cs: state projection/mirror, CausalReceipts, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay receipts + signaling + agent-doc corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 33-39)
- [ ] [#lzcstemporal] lazily-cs: TimerCell/IntervalCell/CronCell/DeadlineCell over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows; replay temporal + rateshape + windowing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 40, 41, 45)
- [ ] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [ ] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
- [ ] [#asyncplanenever] STAGED implementation — finish the async v2 kernel naming migration in C++, Go, Python, and Zig per tasks/plan-lazily-async-v2-migration.md. Rust, JavaScript, Kotlin, and C# already expose AsyncSource/AsyncComputed. Preserve deprecated aliases in published bindings, collapse live memo constructors into guarded computed, and settle the public AsyncComputedState vs formal AsyncSlotState distinction without renaming the formal model mechanically.
- [ ] [#lazilyconformancecoverag] lazily-cs: STAGED per operator — incomplete C# surfaces stay out of peer/feature test groups until implemented. Fresh runtime measurement 2026-07-27: 48/124 canonical fixtures opened, 76 known-uncovered (reactive-graph 21, collections 10, statechart 7, materialization 3, familysync 1, QueueCell 5, WorkQueueCell 2, and TopicCell 4 are covered). The remaining fixtures are genuinely missing product surfaces, not runner-wiring gaps. Keep open and work it down only by shipping the explicit tasks in tasks/software/lazily-cs-parity-plan.md (#lzcscollections, #lzcsqueues, #lzcscrdt, #lzcsipc, #lzcssync, #lzcstemporal, #lzcscluster, #lzcsplane); admit C# to each capability group only when that production surface and canonical replay ship.
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
