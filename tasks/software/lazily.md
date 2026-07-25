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
/goal Complete all lazily-cs features in the feature table.

### Re: complete all lazily-cs features — opus-5

> **Queue prompt:** /goal Complete all lazily-cs features in the feature table.

One of ten phases is done, not all of them. `#lzcsconcurrency` — rows 5 and 6 — is shipped, proved, and pushed. The other 44 rows are nine more phases and are still queued. I am saying that up front because the alternative is a claim that would not survive contact with the ledger.

**What shipped**

`ThreadSafeContext` wraps the single-threaded kernel in a reentrant lock and reuses its batch coalescing, so it *refines* the kernel rather than reimplementing it — a one-write critical section is observationally a plain `Set`, and three writes in a batch are one cascade. `ThreadSafeKernel.ApplyBatch`/`FlushBatch` port the Lean `ThreadSafe` model as a pure function over a node table, which is what makes the coalescing law checkable without a live graph.

`AsyncContext` is a distinct graph, not an overload: its own handles, the `Empty`/`Computing`/`Resolved`/`Error` state machine, a revision per slot, in-flight deduplication, the five cancellation properties, the `get_async` re-resolve loop, and retained-cleanup effects with serialized reruns over a synchronous batch boundary.

**The part that mattered.** The reactive-graph runner is now parameterised over the execution model and replays the same op stream against all three contexts — 51 fixture replays, 584 assertions, empty ledger. That is the whole reason this phase went first. A cascade that stops one level below the write is *correct* synchronously and *broken* asynchronously: an async read short-circuits on a resolved cache, so the stale value is served forever and no pull chain rescues it. I mutation-checked exactly that — cutting the async walk to one level fails `transitive_invalidation_reaches_depth` plus four other fixtures on `AsyncContext` and nothing at all on the other two models. Dropping the thread-safe batch boundary fails only the `ThreadSafeContext` replays. Neither mutation is visible from the sync plane, which is the point.

**Nine fixture/model pairs are gated, each with a reason.** `AsyncContext` ships no eager slot constructor (3 signal fixtures), no `MergePolicy` (5 merge fixtures), and no bounded effect drain (1) — all three are synchronous-kernel constructs. I gated rather than degraded on purpose: a lazy slot standing in for an eager signal produces plausible numbers that satisfy two of the three assertions a signal fixture makes, so the suite would go green while testing the opposite of the fixture's purpose.

**Three deliberate divergences from lazily-go**, recorded in `AGENTS.md` so they are decisions: serialization is a reentrant lock rather than an owner goroutine; the async invalidation walk is non-consuming, matching this binding's own synchronous kernel, so degree assertions mean the same thing on all three models; and an `Error` slot retries on the next read. That last one is a real disagreement — lazily-go serves the stored error forever, `docs/async.md` says `Error → Computing` on retry. I followed the spec and queued `#lzgoerrorretry` to settle which side is wrong.

56 tests, format clean, CI green on lazily-cs and lazily-spec. Coverage rows 5 and 6 flipped, nine sibling READMEs regenerated one commit per repo, superproject gitlinks committed.

**What is left.** `#lzcscollections` `#lzcsstatechart` `#lzcsqueues` `#lzcscrdt` `#lzcsipc` `#lzcssync` `#lzcstemporal` `#lzcscluster` `#lzcsplane` `#lzcspackage`, in `tasks/software/lazily-cs-parity-plan.md`. Each is at least the size of the one just finished — collections is larger — and each ends the same way: its corpus replays with an empty ledger, or the row does not flip. Collections is next.

### Re: queue + backlog attributes — opus-5

Three of your four tokens are real; one is ignored, and the whole thing is inert right now for a reason that is not in either marker.

**`&lt;!-- agent:backlog priority queue --&gt;` — both valid.** Bare `queue` is the backlog→queue sync attribute in **append** mode (`queue=sync|append|prepend` are the explicit forms). Bare `priority` stable-sorts backlog items by their per-item `priority=<1..9>` token before the sync, and — because this backlog is *also* `queue`-tagged — additionally runs the synced queue through the priority/auto-DAG recompute, so an append-built queue is reordered immediately.

**`&lt;!-- agent:queue preset="#spec-test-commit-push" priority queue --&gt;` — `preset` and `priority` are valid; the trailing `queue` is not.** `queue` is only meaningful on `agent:backlog`/`pending`. On `agent:queue` it is not a recognized key, so preflight warns `misplaced_component_attr` and ignores it — no mutation, but it will warn every cycle until you delete that one token. Drop it and keep `&lt;!-- agent:queue preset="#spec-test-commit-push" priority --&gt;`.

**Nothing is queued, and the markers are not why.** Preflight reported `queue: held 11 backlog id(s) out of explicitly stopped queue binding`. Frontmatter carries `queue: stop`, which resolves the sync policy to `ExplicitlyStopped` and holds every active backlog id out of the mirror. Separately, even without `stop`, the `queue` attribute only *populates* the queue — it never starts the loop. To start it, set frontmatter `queue: start` (alias `go`), or put the `start`/`go` control on the `agent:queue` marker.

**What it will do once started.** All 11 open backlog items sync in as `do [#id]`, in document order, then get the priority recompute. No item carries a `priority=<1..9>` token, so document order stands and the head is `#lzgoerrorretry` — the lazily-go `Error → Computing` spec disagreement — with `#lzcscollections` second. If you want collections first (the exchange says "collections is next"), either move it to the top of the backlog or give it `priority=1`; append-stability means backlog-sourced items land as a group behind any manual queue lines, so the ordering is decided in the backlog, not the queue.

**One thing I am deliberately not touching.** Preflight also flagged a stale `agent-doc` install: the running binary is missing three committed changes, newest `c29e32b7f feat: rebuild request as an Effect over a signal (Tier 2)`. Those commits belong to the concurrent agent-doc session on another pane, and `tasks/agent-doc/agent-doc-bugs2.md` is foreign-owned by pane %1. Building that tree from this document would race a session mid-change, so I left it. That session owns its own `make install`.

**No new backlog items this cycle.** This turn produced no actionable follow-up work of its own — the eleven lazily-cs phases already in `agent:backlog` are the whole remaining plan, and the two things to change (`queue: stop` → `start`, delete the stray `queue` token) are operator marker edits, not tracked work.

Say the word and I will flip `queue: start`, remove the stray `queue` token, and drain — but the drain is ten multi-repo phases with commits and pushes under `#spec-test-commit-push`, so I want the go from you rather than inferring it from an attribute edit.
<!-- agent:boundary:b9ff7b5f -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority queue -->
- do [#lzgoerrorretry]
- do [#lzcscollections]
- do [#lzcsstatechart]
- do [#lzcsqueues]
- do [#lzcscrdt]
- do [#lzcsipc]
- do [#lzcssync]
- do [#lzcstemporal]
- do [#lzcscluster]
- do [#lzcsplane]
- do [#lzcspackage]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzgoerrorretry] lazily-go: an async slot in Error serves the stored error forever, but docs/async.md § Async slot state machine lists `Error → Computing` — "get_async retry after an error". lazily-cs follows the spec; either lazily-go is a finding or the spec sentence is wrong. Decide in lazily-spec first, then fix whichever side loses
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

## Completed / Reaped

<!-- agent:done -->
- 2026-07-25 [#lzcsconcurrency] lazily-cs: ThreadSafeContext (lock-backed) + AsyncContext, and re-parameterise the reactive-graph conformance runner over the execution model so all three replay the same corpus — plan: tasks/software/lazily-cs-parity-plan.md (coverage rows 5-6)
<!-- /agent:done -->
