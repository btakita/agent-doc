# Plan — ReactiveMap unification (Phase A) and family conformance

Status: **in progress.** The spec question at the end is **settled** (see
"Resolved" below); `rs` and `zig` are done; seven bindings remain. Supersedes the
narrower "expose a ctx-taking factory on ReactiveMap" recommendation, which is a
symptom of the structure this plan addresses.

> ## Progress
>
> **Resolved — the open spec question.** `cell-model.md` now defines a **Core**
> surface (REQUIRED of every binding × every flavor, fixture-gated) and an
> **Extended** set (OPTIONAL, non-normative, unscored). Deprecating the drifted
> methods was rejected: it would churn nine released bindings to delete working
> methods that break nothing. The real defect was that nothing distinguished
> "kt is missing seven required methods" from "cs also ships a `reconcile`
> helper" — both read as divergence and both scored green. Ordering and atomic
> move are Core and **bind every flavor**. Kind specializations (`set`
> source-only, `materialize_all` computed-only) bind per entry kind, not per
> flavor. `coverage.json`'s one conflated map row is now four.
>
> **Done — lazily-rs** (`325ee7d`, CI green). `KeyedOrder<K,H>` core; thread-safe
> and async gained `position`/`move_*`/`remove`; the tracking view is threaded
> into the factory on all three (async's trait compute widened from a nullary
> `Arc<dyn Fn() -> V>`); `AsyncContext::eval_detached` + `is_set`; `PartialEq`/
> `Debug` on the async handles. Both fixtures replay against all three flavors,
> mutation-checked three ways.
>
> **Done — lazily-zig** (`5327635`, `529b20c`). Shared `KeyedOrder`; thread-safe
> and async gained the full Core surface; both fixtures replay against all three
> flavors. Sync stays **partial**: entries are cached values, not nodes, so
> `handle` is a birth id — enough for the atomic-move law, no dependents or
> lineage. Making it green means making the map graph-backed.
>
> **Corrections this work forced:**
> 1. **The `remove` blocker was wrong.** "TS and async handle traits have no
>    disposal hook, so a flavor `remove` is handle-trait surgery" — both rs
>    contexts already expose `dispose_cell`/`dispose_slot`, stronger than the
>    sync `clear_dependents` and observably the same.
> 2. **A defect the survey missed: zig's `moveBefore`/`moveAfter` land the key on
>    the wrong side of the anchor** whenever it precedes the anchor.
>    `moveBefore("a","d")` on `[a,b,c,d]` gave `[b,c,d,a]`. Both computed the
>    target on the pre-removal list. Verified by probe before changing anything.
> 3. **The one place rs *did* replay the contract wasn't asserting it.**
>    `collections_conformance.rs` read `invalidates` off the step, but the
>    fixtures nest it under `expected` — always `None`, so `assert_invalidation`
>    never ran. **Check this in the seven remaining bindings.**
> 4. **zig's `moveTo` desync is fixed structurally, not on its error path.**
>    Reordering cannot need memory, so the move is a `std.mem.rotate` and there
>    is no allocating error path left to desync on.
>
> **Remaining:** py, js, dart, cpp, cs, go, kt — Core surface per flavor, plus
> the fixture replay. Sequencing below still holds, minus zig.

> ## Corrections after adversarial review
>
> An independent review refuted four load-bearing claims in earlier drafts.
> Corrected below; recorded here because the errors shaped the plan's framing.
>
> 1. **"Nine of nine bindings VIOLATE the spec" — withdrawn.** `cell-model.md:406-408`
>    scopes flavor obligations: "A flavor MUST preserve the **entry-kind and
>    materialization laws** above; it only adds the context-specific guarantee."
>    Ordering and atomic move are not flavor obligations, and `:454-458` requires
>    "**at least** its `SourceMap` specialization". The capability gap is real; it
>    is **not** a spec violation. Whether it *should* bind every flavor is the open
>    spec question at the end of this document — it was never settled text.
> 2. **"All three families store the same closure-free state" — false.** Sync
>    `ReactiveMapInner` additionally holds `membership: Source<u64>` (`cell_family.rs:210`)
>    and `order_signal: Source<u64>` (`:218`) — **graph-bound reactive handles**.
>    TS and async hold only `materialized + order`.
> 3. **"The ordering plane touches no handle" — false.** `move_*` never touch `H`,
>    but the version bump is `ctx.set(&self.inner.order_signal, …)` (`:255-259`,
>    called from `move_to` at `:389`) — a handle write, sync-context-typed. So a
>    *graph-agnostic* ordering core is not achievable; a graph-agnostic core is
>    precisely what zig's sync map already is, which this document catalogues as a
>    defect ("u64 counters callers must poll").
> 4. **The `remove` exclusion was over-cautious with wrong formal backing.**
>    `materialize_present_comm` is *confluence*; monotonicity is
>    `materialize_present_monotone`. The Lean models contain **no remove operation
>    at all**, so no theorem constrains adding one. The same fixture replays
>    against the sync map, which ships `remove`. And lazily-cs's
>    `AsyncComputedMap.Remove` already exists. Monotonicity is a materialization-plane
>    law (lazy allocation never *silently* drops), not a ban on an explicit removal API.

## Why

`get_or_insert_with` hands its factory only `&K`. Two of the three lazily-rs map
families already receive a tracking view and **explicitly discard it**:

| family | file | `mint_with` compute param | discard site |
|---|---|---|---|
| sync | `src/cell_family.rs` | `Fn(&Compute) -> V` | `move \|_ctx\| factory(&k)` — `:305` |
| thread-safe | `src/thread_safe_reactive_family.rs` | `Fn(&ThreadSafeContext) -> V` | `move \|_ctx\| factory(&k)` — `:203` |
| async | `src/async_reactive_family.rs` | `Arc<dyn Fn() -> V>` — **nullary** | `move \|_actx\| …` — one layer lower, in the handle impl (see "Async — corrected") |

So a derived entry's factory cannot read another reactive and have the read
register a dependency edge. The fix is to stop dropping the parameter — but the
same investigation surfaced a larger problem.

## The real finding: the flavors have drifted, and two violate the spec

Public method surface per family:

| family | methods | ordering / atomic move |
|---|---|---|
| `ReactiveMap` (sync) | 22 | `move_to`, `move_before`, `move_after`, `position`, `keys`, `remove`, `len`, `contains_key` |
| `ThreadSafeReactiveMap` | 11 | **absent** |
| `AsyncReactiveMap` | 10 | **absent** |

`cell-model.md:272-276` requires collections of every *binding* — satisfied by
each binding's sync map. It does **not** require ordering of every *flavor*:
`:406-408` scopes flavor obligations to the entry-kind and materialization laws
plus one context-specific guarantee, and `:454-458` sets the floor at "at least
its `SourceMap` specialization".

So this is a **capability gap, not a spec violation**. It went unnoticed because
the ordering fixtures only ever replay the single-threaded flavor — the same
blind spot as the marker-only conformance runners found in the EntryKind cycle:
the matrix reads OK because *a* flavor passes. Whether the contract *should*
bind every flavor is an open spec question (see the end of this document).

## Key structural fact

All three families retain **no closure, factory, or context**. But the state is
**not** identical, and the difference is load-bearing:

| family | state |
|---|---|
| sync (`cell_family.rs:207-221`) | `entries`, `order`, **`membership: Source<u64>`**, **`order_signal: Source<u64>`**, + two `StdCell<u64>` mirrors |
| thread-safe (`:112-118`) | `materialized`, `order` |
| async (`:103-106`) | `materialized`, `order` |

The sync map's two extra fields are **graph-bound reactive handles**. Membership
and order *invalidation* is `ctx.set` on them (`:255-271`), and the reactive
reads `keys` / `len` / `contains_key` subscribe through them (`:339-341`,
`:431-434`, `:443-446`).

**Consequence:** the bookkeeping half (the key list, `position`, the move
algebra) is genuinely graph-agnostic, but *reactivity* over it is not. A core
that is graph-agnostic all the way is exactly zig's sync map — which this
document catalogues as a defect, because its "signals" are `u64` counters
callers must poll.

## Why not unify over `SyncReactiveGraph`

The obvious move is to write one `ReactiveMap<G: SyncReactiveGraph, K, V, H>`.
The seam exists and is implemented for both contexts:

```
src/reactive_graph.rs:183   type Compute<'a>            // GAT
src/context.rs:2699         type Compute<'a> = Compute<'a>;
src/thread_safe.rs:3764     type Compute<'a> = ThreadSafeContext;
```

`&G::Compute<'_>` is therefore a single spelling that projects correctly per
execution model, and `computed`/`effect` already take exactly it.

**But the trait imposes `Send + Sync` that the concrete single-threaded API does
not.** Compare:

| | value bound | closure bound |
|---|---|---|
| `Context::computed` (`src/context.rs:840`) | `T: PartialEq + 'static` | `F: Fn(&Compute) -> T + 'static` |
| `SyncReactiveGraph::computed` for `Context` (`:2707`) | `T: PartialEq + Send + Sync + 'static` | `F: ... + Send + Sync + 'static` |

Routing the map through the trait would newly require `Send + Sync` on
single-threaded map values and factories. A single-threaded caller capturing an
`Rc` — the normal case — stops compiling. An impl cannot weaken a trait's
bounds, so this cannot be patched at the impl.

Corroborating, and stronger than an earlier draft claimed: `SyncReactiveGraph`
has **zero** generic consumers anywhere in `src/` or `tests/` — only the trait
definition, two impls, and a `lib.rs` re-export. (A draft cited
`tests/reactive_graph/model.rs:226` as a consumer; that line is a *comment*
saying a single engine cannot be generic over both, and `GraphModel` bounds
`ReactiveGraph`, not `SyncReactiveGraph`.) The bound compromise has never been
load-bearing, which is why the mismatch went unnoticed.

## Phase A — extract the closure-free core

Split along the seam the codebase already uses everywhere else (AGENTS.md
describes "pure cores split from reactive cells" for resilience, windowing,
coordination, membership, and service). The map families are where it was not
applied.

1. **`KeyedOrder<K, H>`** — a graph-agnostic, closure-free core owning
   `entries` + `order` and all of: membership, present-set, `position`, `keys`,
   `len`, `contains_key`, and `move_to` / `move_before` / `move_after` with the
   LIS-preserving atomic-move semantics.

   **`remove` — earlier exclusion withdrawn.** A draft barred it from the shared
   core on monotonicity grounds. That was wrong: the cited theorem
   (`materialize_present_comm`) is *confluence*, not monotonicity; the Lean
   models contain **no remove operation at all**, so nothing constrains adding
   one; the same fixture already replays against the sync map, which ships
   `remove`; and lazily-cs's `AsyncComputedMap.Remove` exists today. The
   "present set only grows" line also appears on the **sync** map
   (`cell_family.rs:344-345`) ten lines above a `remove` that shrinks it — the
   doc does not discriminate the flavors. Monotonicity is a materialization-plane
   law (lazy allocation never *silently* drops), not a ban on explicit removal.

   Real blocker instead: `MapHandle::clear_dependents` exists only on the **sync**
   handle trait (`cell_family.rs:134`). The TS and async handle traits have no
   disposal hook at all, so a flavor `remove` is handle-trait surgery.
2. Each family keeps a thin shell holding its own interior-mutability wrapper
   and its own `materialize` / `observe` — the only genuinely per-model parts.
3. Thread-safe and async gain the **bookkeeping** half (key list, `position`,
   move algebra) from the core. They do **not** gain reactivity for free — see
   below.
4. The ctx-taking factory becomes a small independent change per family: pass
   `move |ctx| factory(ctx, &k)` instead of `move |_ctx| factory(&k)`.

This sidesteps the `Send + Sync` problem, because the core never holds a factory
or a tracking view.

### The membership/order-signal plane — the largest unbudgeted piece

TS and async have **no reactive `keys` / `len` / `contains_key` at all**, only
non-reactive `present_keys` / `present_count`. The gap is wider than ordering:
*membership reactivity itself* is absent, and it is what `cell-model.md`
clause 8 means by "a dedicated membership cell tracks the key set".

Closing it requires each flavor to **mint its own membership and order `Source`
cells on its own graph** and route bumps through its own `set`. That is
per-context work the shared core cannot supply, and it is the bulk of the real
effort. "Purely additive, by construction" was wrong.

### What breaks

- `get_or_insert_with` / `get_or_insert_handle` arity changes in sync and
  thread-safe. Published API — every caller updates.
- Nothing else. The core extraction is internal; ordering additions are new API.

### Async — corrected

An earlier draft of this plan claimed async "has nothing to discard" and that
fixing it required giving `AsyncContext` a compute view it lacked. **Both are
wrong.** The discard is real; it just sits one layer lower:

| layer | code | what happens |
|---|---|---|
| map | `Arc::new(move \|\| factory(&k))` | nullary closure — nothing to discard *here* |
| trait | `AsyncMapHandle::materialize(ctx, compute: Arc<dyn Fn() -> V + Send + Sync>)` | **the severing point** — the nullary type stops any view reaching the map |
| handle impl | `ctx.computed_async(move \|_actx\| { let v = compute(); async move { v } })` | **the view exists and is discarded here** |

`AsyncContext::computed_async` takes `F: Fn(AsyncComputeContext) -> Fut`
(`async_context.rs:413`), and `AsyncComputeContext` (`:597`) is a genuine
value-threaded tracking view: it carries `_node_id: SlotId`, a
`dependencies: Arc<Mutex<HashSet<SlotId>>>` edge set, a generation stamp for
disposal safety, and a tracked `get<H: Read<Self>>`.

So async is the **same defect with the same fix shape** as sync and thread-safe.
Phase B is not ambient-removal — it is widening `AsyncMapHandle::materialize`'s
compute type from `Arc<dyn Fn() -> V>` to a view-taking form and no longer
binding `_actx`.

One genuine async wrinkle remains: the wrapper builds a *ready* future
(`let v = compute(); async move { v }`), so a derived entry's compute is
synchronous inside an async shell. Given the view it can **track**; it still
cannot **await**. Those are separable concerns and only the first is needed for
dependency edges.

Async also gains the ordering/atomic-move core in Phase A.

### Verification

- Replay the collections ordering/atomic-move fixtures against **all three**
  flavors, not just sync. That is the regression guard for the gap this closes.
- Full `cargo test --all-features` + `clippy -D warnings`.
- Note: `make check`'s `benchmark-check` target fails in a tree that has never
  run the benches (`no Criterion estimates found`); that is environmental.

## Family conformance

Read-only surveys across all eight sibling bindings. The gap is not a Rust
problem — it is family-wide and identically shaped.

### The gap is universal

Public method surface, sync / thread-safe / async:

| binding | sync | thread-safe | async | ordering + atomic move |
|---|---|---|---|---|
| rs | 22 | 11 | 10 | sync only |
| py | 22 | 10 | 9 | sync only |
| go | 17 | 6 | 8 | sync only |
| kt | 19 | 8 | 9 | `SourceMap` only — **5 of 6 families lack it**, incl. sync `ComputedMap` |
| dart | 24 | 10 | 9 | sync only |
| cpp | 21 | 9 | 8 | sync only |
| js | 17 | 8 | 8 | sync only |
| cs | 20 | 9 | 9 | sync only |
| zig | 22 | 8 | 11 | sync only |

**Nine of nine bindings fail `cell-model.md:275` on the thread-safe and async
flavors.** No binding exposes `move_to` / `move_before` / `move_after` on
anything but its single-threaded source map.

### The contract is verified almost nowhere

- kt, go, py, cpp, dart, cs — the atomic-move fixture is replayed against the
  sync source map only.
- **zig — never replayed at all.** `cellmap_atomic_move.json` and
  `cellmap_independence.json` appear only in doc comments
  (`reactive_map.zig:32,:39`); only `materialization/*.json` is loaded (`:698`).
- **js — never replayed against a reactive map at all.** The collections
  fixtures are loaded only by `test/collections.test.js`, which drives a
  *fourth*, non-reactive model (`src/collections.js`). All three reactive
  families load only `materialization/`.

So the invariant the spec calls "required of every binding" has no executable
gate on two bindings and no coverage of the thread-safe/async flavors on any.

### The three execution models are largely a fiction at the map layer

In six bindings the thread-safe map is the sync graph plus a lock:

- **cpp** — `BasicThreadSafeContext` holds `Context ctx_` (`thread_safe.hpp:329`);
  `AsyncContext` holds `Context ctx_` (`async_context.hpp:182`). A compile probe
  showed `SourceMap::{keys,len,contains_key,get}` already accept a
  `ThreadSafeContext` **unmodified**. Exactly two signatures block the rest:
  `Source::clear_dependents(Context&)` and `Computed::clear(Context&)`.
- **cs** — `ThreadSafeContext.Inner` **is** a `Context`, so the projection is the
  identity. `ThreadSafeComputedMap` is already a 90-line delegating shell over a
  real `ComputedMap`; closing its gap is 8 one-line delegations.
- **dart** — `ThreadSafeContext` wraps a plain `Context`; all three families take
  the same `Context` and every test constructs a bare `Context()`.
- **py** — `ThreadSafeContext` is used for writes only; reads route through
  `_reads` → the sync `Context`.
- **go** — `ThreadSafeContext` wraps a `*Context`; the TS map is a plain
  mutex-guarded value cache with no reactive nodes at all.
- **js** — the map's own mutex guards realm-private state under
  run-to-completion; only the *lock* crosses realms, not the graph
  (`thread-safe.js:134-137`).

### The decisive fact for sequencing

**The ordering plane touches no handle.** `move_to` / `move_before` /
`move_after` / `position` operate solely on the key list plus a version bump.
They are not sync-coloured, thread-coloured, or async-coloured. The async maps
are missing them for no language reason at all.

Therefore: **async can have atomic move in every binding today.** Async's real
blocker is only the *read* surface (eventual transparency, `Task`/`Future`/
`Promise` colouring) — which Phase A does not touch.

### Verified: the async read surface is already synchronous in 8 of 9

An earlier draft of this plan said only zig and cpp could unify all three
families, and that the rest were blocked by async colouring. **That was wrong.**
A targeted verification pass across all nine found:

| binding | sync `observe` | genuinely awaitable read | notes |
|---|---|---|---|
| rs | `Option<V>` | **none on the map** | drives via `AsyncContext::get_async` |
| py | `V \| None` | `resolve` only | map **bypasses `AsyncContext`** — takes `ctx: dict`, mints raw `Cell`/`AsyncSlot` |
| js | `V \| undefined` | `resolve` only | map is wired to `AsyncContext`; runtime flattens Promise factories |
| kt | `V?` | `observeAsync` only | disposal is on the *context*, never the map |
| go | `(V, bool)` — but **not a pure read**: `Observe` takes a factory and *mints*, running it inline for a source map (`async_reactive_map.go:101-110`, `:88`) | **none** | struct has **no context field** |
| dart | `(V?, bool)` | **none** | `_ctx` is annotated `// ignore: unused_field` — dead |
| zig | `?V` | **none** (no language async) | *is* wired to real async nodes; `drive` is a sync queue drain |
| cpp | `optional<V>` | via handle `std::future<T>` | the only genuine awaitability, and it is off the map |
| cs | `TryGet` exists one layer down, **unsurfaced on the map** | `Task` — **two** methods (`GetOrInsertWithAsync`, `TryObserveAsync`) | all work is `Task.FromResult` underneath |

**Seven of nine have a pure synchronous, non-driving map read.** cs has one a
layer down but does not surface it on the map; **go has none** — its `Observe`
takes a factory and mints, so it is not a pure read and needs a *new method with
different semantics*, not a rename. An earlier draft said "codification of what
8 of 9 already do"; that overstated it.

The reason is not language power. **These families were never genuinely async.**
The per-key factory is sync-typed in all nine, and five of nine are not even
connected to their own async graph.

**So all nine can unify.** kt — the binding an earlier draft ruled out — already
ships the target design.

### Divergences to pin in the spec before standardising

1. **Source maps: `resolve` is pure decoration.** py suspends 0 event-loop
   turns, js 1 microtask. Decide whether a source map should expose `resolve`
   at all.
2. **Absent vs pending is conflated** in py and js (`None`/`undefined` for
   both), but distinguishable in go `(V, bool)`, dart `(V?, bool)`, and cs
   `TryObserve`. Pick one.
3. **`resolve`'s factory** is required in py, optional in js (drive-only on a
   present key).
4. **js alone flattens a Promise-returning factory** and genuinely awaits it —
   verified at runtime, undeclared in its `.d.ts`. py stores the un-awaited
   coroutine instead. This is a portability hazard, not a feature.
5. **go's `Observe` mints** — it takes a factory and allocates. A pure
   `observe(key) -> Maybe<V>` drops both the parameter and the side effect.
6. **Invalidation semantics differ**: kt clears the cache on invalidate
   (`state = Empty; value = null`), so `observe` returns absent rather than a
   stale value; cs retains `Value` but gates on `State is Resolved`, so
   last-known-while-stale is unreadable either way. Decide whether eventual
   transparency means "last known" or "currently resolved".

### Language feasibility, easiest first

| binding | mechanism | notes (all 9 CAN unify — see verification above) |
|---|---|---|
| zig | `fn ReactiveMap(comptime Ctx, K, V, kind)`; projections are `Ctx.Handle` / `Ctx.Observed` / `Ctx.Lock` | **yes** — async is a *resolution axis* (`Observed = ?V`), not a colour; Zig has no language async |
| cpp | traits class + the existing duck-typed `Cx` template param; repo already parameterises its TS context on a lock policy | **yes** — needs one added `clear_dependents` on `AsyncSlotHandle` |
| cs | sync↔TS is free (`Context` identity); order core is plain generic delegates| yes — its async read is already sync |
| dart | one extra bounded type param; `Compute` is non-generic so no GAT needed | yes — verified: no `Future` in the async map file at all, and `_ctx` is dead |
| js | duck typing; `ComputeOps` already satisfied by all three contexts| yes — its async read is already sync |
| py | `ctx: Any` already type-erases the projection| yes — its async read is already sync |
| go | closure fields (already proven in-repo) or one extra type param| yes — its async read is already sync |
| rs | closure-free core (NOT the GAT — see the `Send + Sync` mismatch above) | yes — async `observe` returns `Option<V>`, no future on the map at all |
| kt | extra type param + self-bound; `ComputeOps` is hard-bound to `Context`| yes — its async read is already sync |

**zig already contains the proof.** `Engine(comptime Model: type)`
(`reactive_graph_conformance.zig:1206`) is one generic replay engine written
against `SyncModel` / `TsModel` / `AsyncModel` — the exact three execution
models — with the async projection isolated to a single `settle` method that is
`{}` on the other two. The unification is proven in the test harness and not
applied to the library.

### Defects found while surveying (independent of this plan)

- **zig: the sync map is not graph-backed.** It stores plain `V` and fakes
  reactivity with `u64` counters callers must poll; `keys()` / `len()` /
  `contains()` register no edge. The richest surface is the least reactive.
- **zig: `ThreadSafeComputedMap` mints the wrong node kind** —
  `mintLocked` always calls `tsctx.cell`, ignoring `entry_kind`
  (`thread_safe_reactive_map.zig:98`). Its "computed" entries are cells.
- **zig: `moveTo` can desync on allocator failure** — `orderedRemove` then
  `order.insert(...) catch return false` leaves the key in `materialized` but
  absent from `order` (`reactive_map.zig:288-303`).
- **js: two implementations disagree on `moveTo` with a negative index** —
  `reactive-family.js:270` has no lower clamp, `collections.js:196` does.
  Untested in both.
- **cs: `AsyncComputedMap.Len()` is non-reactive despite its doc**; `_membership`
  is written at `:117` and never read anywhere — dead code.
- **cs: `AsyncComputedMap.Remove` never disposes the removed node**, unlike both
  sync clear delegates, leaving readers on a stale cache.
- **cs: no thread-safe or async `SourceMap` exists at all** — conformance papers
  over it with a hand-locked sync map and a bare `Dictionary`.
- **dart: `ComputedMap` entries are `Slot`, not `Computed`** — post-v2 `Slot` is
  the *unguarded* pass-through, so `ComputedMap` lacks the `==` memo guard its
  name implies, visibly through `handle()`.
- **dart: the thread-safe family should not exist** — spec marks thread-safe N/A
  for Dart (single isolate), yet a 182-line duplicate ships that is weaker than
  what it duplicates.

### Recommended sequencing

1. **Wire the collections/atomic-move fixtures into every binding's replay
   harness, against all three flavors.** Do this *first*. Without it the
   unification lands green against nothing — the same trap as the marker-only
   runners. This alone turns the gap from invisible to red.
2. **Prototype in zig and cpp — not lazily-cs.** Both are feature-complete, both
   can unify all three families, both have the seam most of the way built, and
   neither exposes maps on its C ABI. zig additionally already contains the
   proof (`Engine(comptime Model)`).

   **lazily-cs is not a viable prototype target.** It ships **13 of 48 coverage
   rows — 35 absent**, and it has **no thread-safe or async `SourceMap` at all**;
   its conformance substitutes a hand-locked sync map
   (`MaterializationConformanceTests.cs:305`) and a bare `Dictionary` (`:336`).
   Prototyping a *unification* against two families that do not exist would be
   inventing the surface rather than converging one — it cannot surface what
   breaks, because nothing is there to break. Its zero-compat-cost property is
   real but buys much less than it appears to.
3. **go before it hits v1** — still v0.22.0 on an unversioned module path, so
   breaking now costs nothing; after v1 it costs a `/v2` path and every import.
4. **rs, py, js, dart** — sync + thread-safe only.
5. **kt** — no longer last on async grounds. Verification showed its map read is
   **not** suspend-coloured (`AsyncMap.kt:120`) and disposal lives on the
   context, never the map. Its remaining costs are ordinary: no default type
   arguments, and Kotlin 2.0.21 without context parameters.

### Open question for the spec — RESOLVED

`cell-model.md` mandates the observable contract, not an internal factoring.
Whether the *family* converges on one map shape is a spec question about what
`ReactiveMap` is, not nine independent refactors.

- **Does the atomic-move contract bind every flavor, or only the source map?**
  **Every flavor.** Settled by the Core/Extended split in `cell-model.md`
  § "Core surface vs. binding extensions". Ordering touches no entry handle and
  awaits nothing, so it is neither thread- nor async-coloured; rs and zig now
  ship it on all three at no language cost. `coverage.json` scores it per flavor.
- **Is the thread-safe flavor meaningful where the runtime is single-threaded
  (dart isolates, js event loop)?** **Still open.** dart's own spec row says N/A
  while a full duplicate family ships; js's README argues the opposite of its own
  source comment. This one is about whether the flavor should *exist*, not about
  what it must expose if it does — so it does not block the Core work, but it
  should be settled before dart's duplicate family is either extended or deleted.

