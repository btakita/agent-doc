---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 24f78fda-2db5-41b6-8eb1-52772d913df8
agent: claude
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
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

/goal Please create lazily-cs (C#) implementation. Create gh lazily-hub/lazily-cs. Add a submodule to src/lazily-cs. Please ensure lazily-cs is in conformance with lazily-spec + lazily-formal + conformance test suite + added to features table + implements all features on the features table.

'#spec-test-commit-push': update spec + tests. commit + push
<!-- agent:boundary:5e59c997 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- /goal Please create lazily-cs (C#) implementation. Create gh lazily-hub/lazily-cs. Add a submodule to src/lazily-cs. Please ensure lazily-cs is in conformance with lazily-spec + lazily-formal + conformance test suite + added to features table + implements all features on the features table.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#lzcsconcurrency] lazily-cs: ThreadSafeContext (lock-backed) + AsyncContext, and re-parameterise the reactive-graph conformance runner over the execution model so all three replay the same corpus — plan: tasks/software/lazily-cs-parity-plan.md (coverage rows 5-6)
- [ ] [#lzcscollections] lazily-cs: SlotMap / ThreadSafeSlotMap / AsyncSlotMap materialization, ReactiveMap (CellMap/SlotMap), CellTree + reconcile, SemTree, stable-id alignment; replay collections + materialization + familysync corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 1-4, 9-11)
- [ ] [#lzcsstatechart] lazily-cs: flat state machine + Harel statecharts; replay the statechart corpus — plan: tasks/software/lazily-cs-parity-plan.md (rows 7-8)
- [ ] [#lzcsqueues] lazily-cs: QueueCell + QueueStorage, TopicCell, WorkQueueCell, RelayCell with BackpressurePolicy/SpillStore/Transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 12-14, 16)
- [ ] [#lzcscrdt] lazily-cs: TextCrdt + delta sync, SeqCrdt, CrdtTree, LosslessTreeCrdt incl. dotted-frontier anti-entropy and concurrent-merge convergence, LWW/MV registers, PnCounter, CellCrdt; replay crdt-tree + lossless-tree corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24)
- [ ] [#lzcsipc] lazily-cs: Snapshot/Delta/CrdtSync wire types validated against the lazily-spec JSON Schemas, ShmBlobArena, BlobBackend zero-copy transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27)
- [ ] [#lzcssync] lazily-cs: CrdtPlaneRuntime + anti-entropy, resync coordinator, durable OutboxStore, IpcSink/IpcSource + full-duplex SyncDriver; replay reliable-sync + distributed + message-passing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 28-32)
- [ ] [#lzcstemporal] lazily-cs: TimerCell/IntervalCell/CronCell/DeadlineCell over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows; replay temporal + rateshape + windowing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 40, 41, 45)
- [ ] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [ ] [#lzcsplane] lazily-cs: state projection/mirror, CausalReceipts, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay receipts + signaling + agent-doc corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 33-39)
- [ ] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
