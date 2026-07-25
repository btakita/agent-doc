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
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

/goal Please create lazily-cs (C#) implementation. Create gh lazily-hub/lazily-cs. Add a submodule to src/lazily-cs. Please ensure lazily-cs is in conformance with lazily-spec + lazily-formal + conformance test suite + added to features table + implements all features on the features table.

'#spec-test-commit-push': update spec + tests. commit + push

### Re: create lazily-cs (C#) — opus-5

> **Queue prompt:** /goal Please create lazily-cs (C#) implementation. Create gh lazily-hub/lazily-cs. Add a submodule to src/lazily-cs. Please ensure lazily-cs is in conformance with lazily-spec + lazily-formal + conformance test suite + added to features table + implements all features on the features table.

`lazily-hub/lazily-cs` exists, is a submodule at `src/lazily-cs`, and ships a real, spec-conformant reactive kernel. All 48 feature rows are **not** done — that is ten more phases of work, now planned and queued rather than claimed.

**What shipped and is proved**

- **Row 0, reactive graph.** `Source<T>` / `Computed<T>` / `Effect`, value-threaded `Compute` (no ambient stack), non-consuming mark-frontier invalidation, pull-time equality guard, `Eager()`/`Lazy()`, `Batch`, explicit disposal, `TeardownScope` with reverse-creation-order teardown and `Disarm`, edge-degree introspection, and a bounded effect drain that names the offending effect on exhaustion.
- **Row 15, merge algebra.** `KeepLatest`/`Sum`/`Max`/`SetUnion`/`RawFifo`, `Cell ≡ Source<KeepLatest>`, associativity and flag laws under test.
- **Conformance: the whole `reactive-graph` corpus replays with an empty ledger** — 20 fixtures, 215 assertions, zero unsupported, zero known divergences. That includes the five merge-feed fixtures and `feedback_drain_bound_reports_exhaustion`, which lazily-go parks as unsupported. Two things made that reachable: a `Source` already carries its `MergePolicy` in a field, so `merge_cell` needed no new node kind; and I implemented the bounded drain rather than skipping the fixture that needs it.
- 32 tests green, `dotnet format` clean, CI green on the first run.

**On over-mocking — you were right to ask, and one thing was.** The runner drives a real `Context` end to end; nothing about the library is stubbed. But `merges_of` was counted in the *runner*, incrementing before each call to `Merge()` — which is what the reference runner does, and it is weaker than it looks: it counts the runner's intention, not the library's work, so a binding that swallowed the fold entirely would still report the right number. I moved the counter **inside the merge policy's fold**, so `merges_of` now observes folds the library actually performed. I then mutation-checked the suite: breaking `Merge` to keep-latest fails 1 test; cutting the invalidation walk to one level fails 6. It is not vacuous.

**What is queued, not done.** `#lzcsconcurrency` `#lzcscollections` `#lzcsstatechart` `#lzcsqueues` `#lzcscrdt` `#lzcsipc` `#lzcssync` `#lzcstemporal` `#lzcscluster` `#lzcsplane` `#lzcspackage` — one per actionable phase, each keyed to its coverage rows and the corpus it must replay, in `tasks/software/lazily-cs-parity-plan.md`. Concurrency goes first on purpose: the reactive-graph corpus exists partly to catch an async cascade defect that a single-model replay cannot see, so the context kinds are a prerequisite for every later plane rather than a peer.

**lazily-formal** needs no change. It is language-agnostic, and lazily-cs conforms to it through the corpus — `scope_teardown_equals_fold_of_disposals` is the executable form of `disposeScope_eq_disposeAll`, and it passes.

**Coverage matrix.** `coverage.json` gained a `C#` column (rows 0 and 15 ✅, the other 46 `—`), `sync-coverage.mjs` gained `../lazily-cs/README.md` as a target, and all nine sibling READMEs plus `docs/coverage.md` were regenerated and committed one repo at a time. lazily-spec CI is green; the sibling README-only runs were still in progress and are not gating.

Pushed: `lazily-cs` (new), `lazily-spec`, nine sibling READMEs, and the superproject gitlinks.
<!-- agent:boundary:1015e4db -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
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
