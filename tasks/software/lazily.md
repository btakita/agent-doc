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

### Re: queue + backlog attributes — opus-5

Three of your four tokens are real, one was a lint-blocking typo that I removed, and the queue is populated but will not run.

**`&lt;!-- agent:backlog priority queue --&gt;` — both valid.** Bare `queue` is the backlog→queue sync attribute in **append** mode (`queue=sync|append|prepend` are the explicit forms). Bare `priority` stable-sorts backlog items by their per-item `priority=<1..9>` token before the sync, and because this backlog is *also* `queue`-tagged it additionally runs the synced queue through the priority/auto-DAG recompute, so an append-built queue is reordered immediately rather than at the next activation.

**On `agent:queue`, `preset` and `priority` are valid but the trailing `queue` was not.** `queue` is only meaningful on `agent:backlog`/`pending`; on `agent:queue` it is not a recognized key. Preflight warned `misplaced_component_attr`, and the lint gate hard-blocked closeout with `agent-doc/malformed-attr` — "attribute `queue` on `agent:queue` is missing `=value`" — so the cycle could not be sealed until it was gone. I deleted that one token; the marker now reads `&lt;!-- agent:queue preset="…" priority --&gt;`.

**The queue body is already full; the loop is off.** All eleven `do [#id]` prompts are sitting in `agent:queue` in backlog order. But frontmatter carries `queue: stop`, which resolves the sync policy to `ExplicitlyStopped` — preflight reports `held 11 backlog id(s) out of explicitly stopped queue binding`, `queue_active: false`, `queue_drainable_head_count: 0`. The `queue` attribute only ever *populates*; it never starts the loop. Starting it means setting frontmatter `queue: start` (alias `go`), or putting the `start`/`go` control on the `agent:queue` marker.

**Head order, if you start it.** No item carries a `priority=<1..9>` token, so document order stands: `#lzgoerrorretry` first — the lazily-go `Error → Computing` spec disagreement — then `#lzcscollections`. If you want collections first, as the last exchange said, move it to the top of the backlog or give it `priority=1`. Backlog-sourced prompts are append-stable, so ordering is decided in the backlog, not by editing the queue.

**Two environment flags I am not acting on.** The running `agent-doc` binary is missing three committed changes (newest `c29e32b7f feat: rebuild request as an Effect over a signal (Tier 2)`), and the live JetBrains plugin is 0.2.293 against the 0.2.294 this binary ships. Both belong to the concurrent agent-doc session — `tasks/agent-doc/agent-doc-bugs2.md` is foreign-owned by pane %1 — and building that tree from this document would race a session mid-change. I requested `agent-doc admin recycle` (deferred to the next idle boundary) and left the `make install` / plugin update to that session.

**No new backlog items this cycle.** Nothing actionable came out of this turn that is not already tracked — the eleven lazily-cs phases in `agent:backlog` are the whole remaining plan, and flipping `queue: stop` is an operator control, not tracked work.

Give me the go and I will flip `queue: start` and drain. I am not inferring it from an attribute edit, because the drain is ten multi-repo phases that commit and push under your `#spec-test-commit-push` preset.

### Re: async Error → Computing — opus-5

The spec wins, and lazily-go is not the only binding that lost. Three did.

**The survey.** Nine bindings, two behaviors. **Re-spawn on the next read:** lazily-rs (`AsyncSlotState::Error | Empty => spawn_async_compute`), lazily-py (`if self._state is SlotState.ERROR: self._spawn()  # retry`), lazily-js (`// empty or error: spawn a fresh compute`), lazily-kt (`SlotState.Empty, SlotState.Error -> spawnComputeLocked`), lazily-cs, and lazily-cpp (which recomputes unconditionally). **Cache the error and replay it forever:** lazily-go, lazily-dart, lazily-zig. lazily-go's test even names its source — "mirrors Dart getAsync, which throws `_error` in the error state — no auto-retry" — so this propagated from one binding to another rather than being decided three times.

**What settled it was not the vote.** `LazilyFormal/AsyncSlotState.lean` already had `SlotEvent.retry` mapping `error → computing`. The Lean model is the spec's executable form, and a sticky-error binding implements `retry` as the identity on `Error`, so it is not a model of the state machine. The doc's transition list agreed (`Error → Computing` — "get_async retry after an error"). Only the state *table* was ambiguous: "callers receive the error **or** retry on the next `get_async`". That "or" is the whole defect. Three bindings read it as a permission.

**Why sticky is wrong on the merits, not just by authority.** An `Error` slot holds no cached result — no value, no in-flight revision. Caching the error makes a transient failure permanent for the lifetime of the slot: a timed-out fetch or a disconnected peer is unrecoverable through any read path, and `invalidate` does not repair it either (it is a no-op on `Error` in py, and reaches `Empty` in rs — both reach retry through the *read*, not through the dependency change). There is no read a caller can perform to get out.

**What shipped**

- **lazily-spec** `docs/async.md` — the `Error` row now states that waiters on the failed attempt receive its error, that the error is not a cached value, and that the next `get_async` MUST re-spawn. The transition entry says the transition is mandatory, not optional, and records that `invalidate` on `Error` is a no-op because the retry is owned by the read.
- **lazily-formal** — two new theorems. `retry_from_error_recomputes` pins `error → computing` at the slot's current revision; `retry_off_error_is_noop` pins that recovery cannot disturb an in-flight or resolved slot. Collapsing the retry case of `step` to the identity fails the first proof, so the model now distinguishes the two behaviors it previously permitted equally.
- **lazily-go** — `GetAsync`'s `case AsyncSlotError` short-circuit is gone; `Error` falls through to the same spawn path as `Empty`. The `asyncSlot.err` field went with it: the error reaches the waiters of the attempt that produced it, so storing it was dead state that existed only to enable the replay.
- **lazily-dart** — same change in `_awaitResolved`; `_error` deleted (the failure travels through `_inFlight.completeError`).
- **lazily-zig** — `awaitResolved` re-spawns once on entry, so a read still costs exactly one compute and a persistently failing slot cannot loop. `err_value` is cleared only after `enqueueCompute` has actually moved the slot to `.computing`, because a failed enqueue under OOM leaves the slot in `.err` where the loop unwraps it.

**Every new test is asserted on the compute counter, not the returned error.** This is the trap in this particular bug: a sticky binding also "returns boom" on every read — it just runs the body once. An error-only assertion passes against both behaviors, which is exactly how three bindings stayed wrong under green suites. lazily-dart's existing test is the clearest case: its comment claimed "Retry: a getAsync after the failure spawns a fresh computation", but the helper it called built a *different* slot and only proved the runtime survives a failure. It would have passed unchanged either way. Mutation-checked in all four repos — restoring the short-circuit fails `retry GetAsync = (0, boom), want (7, nil)` and `compute calls = 1, want 3` in go, both new dart tests, and both zig tests; collapsing Lean's retry case fails the proof.

**lazily-zig's dropped-edge OOM test had the sticky assumption baked into its tail** — it asserted the slot "stays visibly failed" after the allocator recovered. Its real claim (an OOM must surface rather than silently resolving with a lying dependency set) is unchanged and still asserted. The error read now happens while the allocator is still failing, and recovery is asserted afterwards, including that the recovering recompute rebuilt the edge it had dropped — which is the thing the old assertion made unobservable.

**Verification.** lazily-spec `make check` green (146 schema tests, Lean build, coverage sync). lazily-formal `lake build` green. lazily-go 425 tests + `go vet` green. lazily-dart 522 tests + `dart analyze` clean. lazily-zig `mise run test` green (442 tests). All five repos committed and pushed; superproject gitlinks committed. lazily-spec CI is green on this commit; lazily-go, lazily-dart and lazily-zig CI runs were still in progress and are recorded, not waited on. lazily-formal has no CI workflow.

**One thing this does not do, and it is the reason the bug survived.** There is no conformance fixture for `Error → Computing`. The contract is now pinned by native tests in each binding plus the Lean proof, but the shared corpus never exercised it, which is why six bindings and three bindings disagreed for as long as they did without a single red suite. Adding one means teaching all nine runners an async-failure op, so it is queued as `#lzasyncerrfixture` rather than smuggled in here — a fixture the other eight runners panic on is worse than none.

### Re: conformance fixture for the retry contract — opus-5

Not done. The fixture exists and lazily-spec is green on it, one of nine runners is wired, and I am stopping there rather than reporting a corpus addition that would turn every binding red the moment it is pushed.

**What the survey changed about the plan.** The item assumed the hard part was teaching nine runners a new op. It is not — all nine reactive-graph runners already replay against an `AsyncContext` model, so the async plane was reachable from the corpus all along. The real constraints are different: runners **ignore** a fixture's `model` field and replay every fixture against every model they ship, and the per-fixture ledgers (`reactiveGraphReplayed` / `reactiveGraphUnsupported`) mean a fixture landing in lazily-spec makes all nine bindings fail their corpus-drift check until each one replays or explicitly parks it. So an async-only fixture is not expressible without first extending the other runners' ledgers, and lazily-spec cannot be pushed ahead of the runners.

**So the fixture is model-agnostic, and that is the better contract anyway.** `failed_compute_is_never_cached.json` states the rule without naming a state: a compute that fails is never cached, the next read re-runs the body. On the async plane that is `Error → Computing`; on the sync plane it is the same rule with no state to name. I verified against lazily-go that the sync plane already behaves this way — cached value, invalidate, failing recompute, then a read that recovers: computes go 1 → 2 → 3 and the value comes back as the current dependencies imply, through both `Get` and `TryGet`. So the fixture is not inventing a sync requirement; it is writing down what the bindings already do, which is exactly the ground the async divergence was hiding on.

**Every assertion is on `computes_of`, none on the error.** Fifteen steps: a healthy baseline that proves the counter is meaningful (a warm read does not re-run), a single armed failure with recovery, then a doubly-armed failure proving a persistently failing node runs one compute *per read* rather than one compute total, and a final write proving the failing runs re-registered their edges instead of leaving the node detached. A caching binding raises the same errors at the same steps and is off by one on the counter at every one of them.

**New op: `fail_next`.** `{"type":"fail_next","id":"slot","count":N}` arms the next N computes of an existing node to raise. It creates nothing and touches no dependency set. Registered in `_REACTIVE_GRAPH_OPS`, and `#lzasyncerrfixture` added to `_REACTIVE_GRAPH_TAGS` so the fixture can cite the contract it pins. lazily-spec `make check` is green — 147 tests, up from 146.

**Where it stands, precisely.**

- **lazily-spec** — fixture + op + tag, `make check` green. **Uncommitted.** Pushing it before the runners turns nine repos red.
- **lazily-go** — `fail_next` implemented on both `syncModel` and `asyncModel` (an `armedFailures` map consumed *inside* the compute body, after the counter ticks, so an armed run is counted like any other), `graphModel.failNext` added, the sync read converts the armed panic into a read error, the `error` assertion generalized from its hardcoded `read_after_dispose` string, and the fixture promoted into `reactiveGraphReplayed`. **The suite is red: 20 divergences.** The library is not at fault — direct probes show the correct 1 → 2 → 3 compute sequence with recovery on both `Get` and `TryGet`. It is a wiring bug in my runner change: the counter stalls at 2 and later reads report an error without re-entering the body, which is the runner's read path swallowing something, not the graph caching a failure.
- **The other eight runners** — untouched.

**Nothing is pushed and no CI is red.** The cost is two dirty working trees: `lazily-spec` (fixture + schema test) and `lazily-go` (runner, suite red). Anyone else picking up lazily-go before this resumes will find it failing, so that is worth knowing before the next session touches it.

Resume point: debug the lazily-go read path until the fixture replays clean on both models, then repeat the same ~15-line change across rs, py, js, kt, dart, cpp, zig and cs, then push lazily-spec and all nine together.

### Re: the conformance fixture, landed across all nine bindings — opus-5

`failed_compute_is_never_cached.json` is on spec main and every binding accounts for it. Ten repos committed and pushed; eight CI green on the exact commit, lazily-rs still running, lazily-zig red for a reason that predates this work.

**What the fixture asserts, and why on the counter.** Sixteen steps: a baseline proving a warm read does not re-run (which is what makes the counter discriminating at all), one armed failure with recovery, a doubly-armed failure proving a persistently failing node runs one compute **per read** rather than one compute total, and a final write proving the failing runs re-registered their edges instead of leaving the node detached. Every assertion is on `computes_of`; none is on the error. That is the whole design constraint — a caching binding raises the same error at the same steps and differs only by one on the counter, so an error-only fixture passes against both behaviors, which is exactly how six bindings and three bindings disagreed indefinitely under green suites.

New op `fail_next` arms the next N computes of an existing node to raise. It creates nothing and touches no dependency set.

**Three design constraints the survey turned up, none of which was the one I expected.**

All nine runners already replay against an `AsyncContext` model, so the async plane was reachable from the corpus the whole time — teaching nine runners a new context was never the problem. The real constraints were: runners **ignore** a fixture's `model` field and replay everything against every context they ship; the per-fixture ledgers make a new fixture fail all nine corpus-drift checks until each binding replays or explicitly parks it; and an armed failure needs an invalidation to force the recompute it fails, which makes the fixture sensitive to eager-vs-lazy scheduling. So the fixture is model-agnostic and names no state — on the async plane the rule is `Error → Computing`, on the sync plane it is the same rule with no state to name. I verified the sync behavior already held before writing it down, so this records what the bindings do rather than inventing a requirement.

**Two engines were reporting the defect the fixture exists to catch, against correct bindings.** lazily-go's `readID` and lazily-py's `read_id` latch an id as permanently unreadable after its first failed read. That was written when disposal was the only error a read could produce, and disposal *is* permanent — but a `fail_next` failure is recoverable by contract, so the latch froze the compute counter and manufactured a divergence. lazily-rs had the same shape one layer down: `quiet` collapsed every panic into `Err(())`, which the engine then latched. It now re-raises the armed sentinel for the engine to convert. Same fix in three languages, same root cause: conflating "this read failed" with "this node is torn down."

**Four bindings needed no library change** — js, py, kt, cs already re-spawn — which is the check that the fixture does not false-positive on correct behavior.

**Three honest per-model skips, each naming a capability gap rather than faking a pass.** `lazily-zig/ThreadSafeContext`: `ComputeFn(T)` returns a plain `T` with no error channel, a deliberate design choice documented in `thread_safe_context.zig`. `lazily-zig/AsyncContext`: no lazy mode for derived slots, so `setCell` enqueues every dependent unconditionally and the armed run is consumed by the write instead of the read — the same eagerness already recorded for `dispose_signal_reverts_to_lazy`. `lazily-rs/AsyncContext`: the compute returns `BoxFuture<Output = i64>` with no error channel and the model blocks on its own runtime inside `read`, so an armed panic kills the spawned task and the read never resolves — the test hangs rather than reporting. In each case the alternative was to return a sentinel value and report an error from `read`, which would cache the sentinel, stop the node re-running, and report a runner artifact as a finding against a correct binding. Both zig and rs gained a per-model skip ledger asserted in both directions, so an entry that stops applying fails the build.

**Verification.** Mutation-checked in five bindings: restoring the sticky-error path fails the fixture on `AsyncContext` and nowhere else in go; the py, dart, kt and cs equivalents each fail their suite. Full suites green — go 427, js 481, py 521, dart 522, kt 349, cpp 39/39 ctest, rs conformance including the async feature, zig 442, cs 56, spec 147.

**lazily-zig CI is red and it is not this change.** `src/lazily/context.zig:2208` uses `builtin.Type.Fn.param_types`, removed from newer `std`; CI pins Zig 0.15.2 and 0.16.0 while the code targets what local dev runs (0.17.0-dev, where the whole suite passes). The identical error is on the commit before mine and the file was last touched by the cell-kernel work. Filed as `#lzzigcitoolchain` rather than waived — a red CI that nobody owns stops being informative.

### Re: zig nightly support — opus-5

> **Queue prompt:** do [#lzzigcitoolchain]

Fixed, and the matrix already had nightly — it just was not gating, which is why the breakage landed in the first place.

**What was actually red.** One line. `normalizeCompute` in `src/lazily/context.zig` read `@typeInfo(F).@"fn".param_types`, and that field only exists on nightly: 0.17.0-dev renamed `params: []const Param` (whose elements carry `.type: ?type`) to `param_types: []const ?type` plus a parallel `param_attrs`. So the code compiled on the machine it was written on and broke the 0.16.0 gate. Every one of the "214 errors" / "54 errors" / "64 errors" counts in the CI log was that same line, re-reported per test target.

**Both shapes are now read behind `@hasField`.** The condition is comptime-known, so the branch not taken is never analyzed and one source tree satisfies both toolchains — no per-version file, no build flag.

**On your point about supporting nightly: it was already in the matrix, and that was the problem.** `master` ran with `continue-on-error`, on the reasoning written into the workflow — a main branch red from an upstream commit nobody here made is a branch people learn to ignore. That reasoning inverts once development happens *on* nightly: it becomes the toolchain most exercised and least likely to be noticed when it rots, and the practical cost of not gating it was nightly-only code reaching main and breaking the **0.16.0** gate instead. So `master` is promoted to a gate alongside 0.16.0. If the upstream-churn worry outweighs that for you, it is one line in `continue-on-error` to put back.

**0.15.2 stays non-gating and still does not compile.** Its blockers are unrelated and pre-existing: `std.testing.io` in `reliable_sync.zig`, a comptime dependency loop in `cell.zig`, and an unresolvable comptime value in `cell_0_15_test.zig`. I left the job in place so the gap stays visible rather than being quietly dropped, and did not fold it into this fix.

**Verified against the real toolchains, not inferred.** All three were already installed locally, so I built and ran the full suite on each rather than reasoning about std diffs: 0.16.0 PASS, 0.17.0-dev PASS, 0.15.2 FAIL on the pre-existing blockers above. I also reproduced the original 0.16.0 failure locally first, so the fix is tied to the actual error rather than to the CI log.

Pushed; the run is queued. This also un-reds the one repo that was still failing after the conformance work — every other lazily repo is green on its exact commit.

### Re: lazily-cs keyed collections — opus-5

Row 1 shipped and pushed. Six of the phase's seven rows are not done, and the phase itself turned out to be mis-shaped, which is the more useful finding.

**The phase bundles four independent surfaces.** Mapping fixtures to rows rather than trusting the phase table: the `materialization` corpus (3 fixtures) is rows 1–3, `familysync` (1) is row 4, and only **5 of the 20** `collections` fixtures belong here at all — `cellmap_atomic_move`, `cellmap_independence`, `keyed_reconciliation_lis` for row 9, `semtree_incremental` for row 10, `stableid_alignment` for row 11. The other fifteen are `#lzcsqueues` and `#lzcscrdt` work. So `#lzcscollections` was one id covering a materialization port, a thread-safe/async port, a sync plane, a tree reconciler, and a memoized semantic tree. I split it rather than gate it repeatedly.

**What shipped.** `ReactiveMap<TKey,TValue,THandle>` — the shared core, ported from lazily-go's `reactive_map.go` — plus `CellMap` and `SlotMap`. Three independent reactive planes on one map: writing an entry invalidates only that entry's readers, add/remove bumps membership and order, and a move bumps **order only** while keeping the entry's handle, dependents, and lineage. A reorder is not a remove plus a re-mint, and keeping those on separate signals is the only reason that distinction is observable.

No eager/lazy mode flag, matching the family: eager is `MaterializeAll` (a pre-mint loop), lazy is `GetOrInsertWith` (mint-on-access). That is what makes "materialization changes allocation timing, never observed values" checkable rather than asserted.

**The materialization corpus replays with an empty ledger.** Every fixture pins the **present set** as well as the values, and that is the whole design of the runner: the value laws alone pass trivially against a map that ignores the strategy and pre-mints everything — that binding gets every `observe` assertion right and every `present` assertion wrong.

**The mutation check is the part worth reporting, because my first two attempts did not go red.** Removing the warm-key guard from `GetOrInsertWith` — suite still green. Removing it from `MintWith` instead — still green. Neither is a vacuous test; the law is defended by **two independent guards**, so a single-point mutation is masked by the other. Removing both, which is what a binding that re-mints on every read actually looks like, fails `present_after_each_read`: it climbs 1,2,3,4 where the fixture pins 1,2,2,3. I would have recorded a false "mutation-checked" if I had stopped at the first attempt.

**Verification.** `make check` green (57 tests, up from 56), `dotnet format --verify-no-changes` clean. Coverage row 1 flipped for C#, `sync-coverage.mjs` run, and the regenerated table committed one repo at a time across the spec and all nine sibling READMEs — `coverage.json` is a hot multi-session file. lazily-cs CI was queued at closeout, not yet green; recorded rather than waited on.

**What is left, now as three actionable items instead of one.** `#lzcscollections` narrows to rows 2–4 (`ThreadSafeSlotMap`, `AsyncSlotMap`, keyed-map sync + the `familysync` corpus), `#lzcsreactivemapfx` takes row 9 (the three CellMap/reconciliation fixtures plus `CellTree`), and `#lzcssemtree` takes rows 10–11 (`SemTree` and stable-id alignment). Each is one surface with its own fixtures, so each can close on an empty ledger instead of accumulating partial credit against a parent.

### Re: keyed reactive maps + CellTree — opus-5

> **Queue prompt:** do [#lzcsreactivemapfx]

Row 9 done, pushed, coverage flipped. The three `Collection`-kind fixtures replay with an empty ledger.

**What the runner actually asserts.** Not values — **invalidation**. Three real reader slots stand on the map, one per reactive plane (a value reader per key, a membership reader over `Len`, an order reader over `Keys`), each counting its own recomputes from inside its compute body. That placement is the whole design: a map that bumps one shared signal for every mutation returns exactly the right order, values, and membership at every step and gets every `invalidates` assertion wrong. A value-only runner cannot tell the two apart.

**One interpretation call, and it is worth stating.** Step 2 of `cellmap_independence` removes `b` and expects `value: []`. My first run reported `b` as invalidated — correctly, because `CellMap.Remove` clears the orphaned cell so its readers learn their source is gone rather than sitting on a stale cache. The fixture's wording is "insert/remove MUST NOT invalidate **unrelated** entry value readers", so the removed key is not in scope. Value probes are now scoped to keys still present. That is a runner fix, not a relaxed assertion: the removed key's own reader is *supposed* to be invalidated, and counting it would have reported intended behaviour as a divergence.

**Reconcile.** `Insert`, `Reconcile`, the four-way `DiffOp` union, and a patience-sort LIS ported from the sibling reconcilers. Reconciling `[a,b,c,d] → [b,c,a]` emits exactly `{remove d, move a}`: `b` and `c` are already in relative order, so holding the LIS fixed means nothing touches them, and their value cells survive a sibling reorder untouched.

**CellTree, and why I did not settle for a partial row.** Row 9 names `CellTree`, and no fixture in the corpus reaches it — the collections fixtures drive `CellMap` directly. The plan's rule would have allowed `~` with a stated gap. It is a thin composition though (a value cell plus a child `CellMap`, inheriting every guarantee rather than reimplementing one), so porting it and marking the row shipped was cheaper than writing down why it was partial. It carries native tests instead of corpus coverage, each naming the mutation it dies under — the headline one being that a sibling reorder keeps the moved child's identity and leaves readers beneath it untouched, which is the entire reason a tree is not just nested maps.

**Mutation-checked three ways, each restored.** Collapsing order into membership fails the independence fixture. Making the LIS return nothing fails `keyed_reconciliation_lis` — `b` and `c` move when they must not. Making a move bump membership fails both the fixture and the CellTree test.

**Verification.** `make check` green (61 tests, up from 58), `dotnet format --verify-no-changes` clean. Coverage row 9 flipped for C#, table regenerated and committed one repo at a time across the spec and all nine sibling READMEs. lazily-cs CI was queued at closeout — recorded, not waited on.

Remaining in this phase: `#lzcscollections` (rows 2–4 — `ThreadSafeSlotMap`, `AsyncSlotMap`, keyed-map sync + the `familysync` corpus) and `#lzcssemtree` (rows 10–11).
<!-- agent:boundary:b1d49619 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lzcsreactivemapfx]~~
- do [#lzcssemtree]
- ~~do [#lzzigcitoolchain]~~
- ~~do [#lzgoerrorretry]~~
- ~~do [#lzasyncerrfixture]~~
- ⏭️ do [#lzcscollections]
- ⏭️ do [#lzcsstatechart]
- ⏭️ do [#lzcsqueues]
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
- [ ] [#lzcssemtree] lazily-cs: rows 10-11 — memoized semantic tree (SemTree) and stable-id alignment (manufactured identity); replay collections/semtree_incremental.json and collections/stableid_alignment.json — plan: tasks/software/lazily-cs-parity-plan.md
- [ ] [#lzcscollections] lazily-cs: rows 2-4 — ThreadSafeSlotMap + AsyncSlotMap materialization and keyed-map sync (membership propagation, materialize-on-ingest, derived alignment); replay the familysync corpus. Row 1 (ReactiveMap core + CellMap/SlotMap + materialization corpus) shipped 2026-07-25 — plan: tasks/software/lazily-cs-parity-plan.md
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
- 2026-07-25 [#lzgoerrorretry] lazily-go: an async slot in Error serves the stored error forever, but docs/async.md § Async slot state machine lists `Error → Computing` — "get_async retry after an error". lazily-cs follows the spec; either lazily-go is a finding or the spec sentence is wrong. Decide in lazily-spec first, then fix whichever side loses
- 2026-07-25 [#lzasyncerrfixture] lazily-spec: land the async Error -> Computing conformance fixture. IN PROGRESS, uncommitted: conformance/reactive-graph/failed_compute_is_never_cached.json + fail_next in _REACTIVE_GRAPH_OPS + tag in _REACTIVE_GRAPH_TAGS (lazily-spec make check green, 147); lazily-go runner has fail_next on both models but its suite is RED with 20 divergences from a runner read-path wiring bug (the library is correct - probes show computes 1->2->3 with recovery on Get and TryGet). Eight runners untouched. Do NOT push lazily-spec before all nine runners replay it or every binding's corpus-drift check goes red
- 2026-07-25 [#lzzigcitoolchain] lazily-zig: CI is red on both pinned toolchains (Zig 0.15.2 and 0.16.0) because src/lazily/context.zig:2208 reads builtin.Type.Fn.param_types, removed from newer std. Local dev runs 0.17.0-dev where the full suite passes, so the red is a pin/target mismatch, not a code defect. Either move the CI matrix to the toolchain the code targets or make that reflection version-agnostic. Predates the conformance work: the same error is on the prior commit
- 2026-07-25 [#lzcsreactivemapfx] lazily-cs: row 9 — CellTree + reconcile on top of the shipped ReactiveMap core; replay collections/cellmap_atomic_move.json, cellmap_independence.json, keyed_reconciliation_lis.json (an atomic move must bump order only and keep the entry handle, which is what separates a reorder from a remove+re-mint) — plan: tasks/software/lazily-cs-parity-plan.md
<!-- /agent:done -->
