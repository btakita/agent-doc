# Design: the Cell kernel — `SourceCell` / `FormulaCell`, and the names that go away

**Status:** designed 2026-07-20. **Supersedes
[`lazily-memo-rename-plan.md`](lazily-memo-rename-plan.md)** rather than amending
it. That plan renamed `Slot` to `Memo`; this one deletes `Slot` along with five
other names, and the questions that plan could not answer (§2's `Memo`-vs-
`Computed`, §13's wire dilemma, the `memo()`/`computed()` guard collision) do not
arise here.

**Origin.** The rename plan's own premises failed under execution. `Memo` reads
wrong on the wire, where variants name *things* (`CellSet`, `SlotValue`,
`NodeAdd`) and a caching behaviour is not a thing. Pulling that thread gave the
real shape: if `Cell` + the derived type is meant to read as a spreadsheet —
which is the rename plan's own §3 argument — then the two-type split is what is
wrong, not the second type's name.

---

## 1. The kernel

```
Cell<T, K>                    genus — a node with a readable value
├─ SourceCell<T, M>           written from outside; folds under policy M
└─ FormulaCell<T>             computed from upstream

Effect                        no value; sink — outside the hierarchy
```

Both names answer the **same question**, which is why the pair works:
[`lazily-memo-rename-plan.md`](lazily-memo-rename-plan.md) §5.1 states the
partition as *"a node's value comes either from outside it or from upstream of
it."* `SourceCell` — from outside. `FormulaCell` — from upstream, via a formula.
Same axis, both sides, no leftover.

`Effect` stays outside and cannot be folded in: it has no value to read, so
nothing can ever depend on it. That is a real boundary, not a naming accident —
the sink position is a capability, not a current degree.

### 1.1 Name ledger

| deleted | replaced by |
|---|---|
| `Reactive<T>` (trait) | `Cell<T, K>` |
| `Source<T>` (trait) | inherent impl on `Cell<T, Source<M>>` |
| `MergeCell` | `SourceCell<T, M>` with `M ≠ KeepLatest` |
| `Slot` *(as a reactive value)* | `FormulaCell<T>` |
| `Signal` | `FormulaCell<T>` + puller `Effect` |
| `SlotHandle<T>` | `Cell<T, Formula>` |
| `SignalHandle<T>` | `Cell<T, Formula>` + effect |
| `MergeCellHandle<T, M>` | `Cell<T, Source<M>>` |
| `CellHandle<T>` | `Cell<T, Source<KeepLatest>>` |

Added: `FormulaCell`, `Formula`, `Cell<T, K>`. **Nine names out, three in.**

**`Slot` is not deleted — it is repurposed** (§5.0). It stops naming a reactive
value and starts naming the *storage position that holds a node*, which is what
the arena already is. So `SlotId`, `SlotValue`, and the slab vocabulary all stay
and become accurate rather than legacy. Only the *reactive-value* sense of `Slot`
goes, and it goes to `FormulaCell`.

`Cell ≡ MergeCell<KeepLatest>` stops being a spec assertion and becomes a type
alias with a default parameter.

---

## 2. `Reactive<T>` is already vestigial — measured

The argument for deleting it is not aesthetic. **`Reactive<T>` is implemented for
four handle types and used as a generic bound nowhere.**

All 13 occurrences in `lazily-rs`: the trait definition (`merge.rs:263`),
`Source<T>`'s supertrait clause (`:289`), four impls (`CellHandle` `:297`,
`SlotHandle` `:313`, `SignalHandle` `:319`, `MergeCellHandle` `:325`), and doc
comments. `rateshape.rs` says "over any `Reactive<T>` source" three times in
prose and **does not take the bound** in any signature.

So the trait's entire job today is asserting *"these four types read alike."*
Collapse the four into one and the assertion is vacuous. `Reactive<T>` is a
consequence of having four handle types, not a reason to keep them.

**No trait-object need.** `grep 'dyn Reactive'` returns nothing. Where the
codebase needs heterogeneity it uses an enum, not `dyn` — the conformance
runner's `Ref<G>` is exactly that. The one thing a generic type could not serve
turns out not to be required.

---

## 3. Mechanism: write protection without a trait

The kind is a type parameter, and the write methods live on an inherent impl
restricted to the source instantiation:

```rust
pub struct Cell<T, K> { id: GraphNodeId, _k: PhantomData<(T, K)> }

impl<T, K> Cell<T, K> {
    pub fn get(&self, ctx: &Context) -> T { … }          // every kind reads
}

impl<T, M: MergePolicy<T>> Cell<T, Source<M>> {
    pub fn set(&self, ctx: &Context, v: T) { … }         // only Source writes
    pub fn merge(&self, ctx: &Context, op: T) { … }
}

pub type SourceCell<T, M = KeepLatest> = Cell<T, Source<M>>;
pub type FormulaCell<T>                = Cell<T, Formula>;
```

`formula_cell.set(…)` fails with *"no method found"* — a compile error with no
trait in sight. The policy `M` lives **inside** `Source<M>`, so it exists exactly
where writes exist and is absent on the formula side, instead of being a third
parameter every signature must spell.

Generic code takes `Cell<T, K>`. Humans write `SourceCell` / `FormulaCell`. Both
the uniformity and the readable names, without `Reactive<T>`.

---

## 4. Portability — where this does not hold

Restricting methods by type parameter is the load-bearing trick, and it does not
port everywhere. **This is a trait/interface hierarchy question, not a class
inheritance one** — five of eight bindings have no inheritance at all.

| binding | kind-restricted methods | note |
|---|---|---|
| rs | ✅ | inherent impls on a specific instantiation |
| zig | ✅ | comptime |
| cpp | ✅ | concepts / partial specialization |
| kt | ✅ | extension functions declared on `Cell<T, Source<M>>` |
| **go** | ❌ | **generics cannot restrict methods by type parameter, and methods cannot introduce their own** |
| py / dart / js | n/a | no compile-time enforcement either way |

**DECIDED 2026-07-20 — Go uses an interface, to keep the compile error.**
Runtime rejection was rejected: the whole point of putting the kind in the type
is that `formula.set()` fails to build, and a binding that downgrades that to a
panic gives up the guarantee while paying all the migration cost.

The interface *is* the genus, so nothing new is invented:

```go
type Cell[T any] interface { Get(ctx *Context) T }

type SourceCell[T any, M MergePolicy[T]] struct{ … }  // has Set/Merge
type FormulaCell[T any]                   struct{ … }  // has neither
```

`formulaCell.Set(…)` is undefined at compile time. Go reintroduces the read
abstraction the other bindings delete, but under the name `Cell[T]` — the same
word the genus carries everywhere else, so this is a mechanism difference, not a
vocabulary fork.

Note `lazily-go` is already blocked on a consuming cascade (`#lzsignaleager`), so
sequence this after that resolves rather than stacking two migrations.

---

## 5. Measured carve-outs

Findings from executing the superseded plan. Each one compiles, or would have,
which is why none of them would surface on their own.

### 5.0 `Slot` is repurposed as the storage concept — `SlotId` stays

**DECIDED 2026-07-20.** `Slot` no longer names a derived reactive value; it names
**the position that holds a node**. `SlotId` therefore stays, and is now accurate
rather than legacy.

The code is already a slot map:

```rust
nodes: Vec<Option<Node>>,     // each index is a slot; Option = occupied | vacant
free_ids: Vec<u64>,           // LIFO recycling
```

`src/context.rs:120` already frames it this way — *"`SlotId`s are recycled
(`free_ids` is LIFO)"*. And the slab sense is **unoccupied**: zero hits for
`free_slot`, `slot_index`, `slots[`. The superseded plan checked for exactly this
and read the result as "the rename is clean"; it is better read as *the storage
meaning is free to take*.

**The ecosystem collisions that killed `Slot` dissolve under the new meaning:**

| ecosystem | means | under "storage slot" |
|---|---|---|
| Python `__slots__` | fixed attribute storage | **aligned** — the same idea |
| Vue / Svelte `<slot>` | a position content is inserted into | **aligned** — a position holding something |
| Qt signals & slots | a callback | evaporates — the joint reading requires `Signal`, which is deleted |

`Slot` was wrong for *"a derived reactive value."* It is right for *"a position
that holds a node."*

**It also beats every alternative considered**, because a slot holds *any* node
kind — `SourceCell`, `FormulaCell`, or `Effect`. The container's name does not
depend on its occupant, which is exactly what defeated `CellId` (§5.1) and
`NodeId` (§5.1).

**Consequence: `SlotValue` on the wire is correct, not a seam.** It is the value
in a slot. The superseded plan's §13 dilemma — leave a permanent seam, or take a
breaking wire change across eight bindings **plus `agent-doc`'s IPC** — simply
does not arise. No rename, no dual-accept.

Layering: a `Cell<T, K>` handle carries a `SlotId`; the slot holds a node. The
`recycled_id_inherits_nothing` fixture becomes vividly named — the slot persists,
the occupant changes.

### 5.1 Superseded by §5.0 — retained for the reasoning

`lazily-memo-rename-plan.md` §10 step 6 carves out `SlotId`, correctly: it is the
id of **every** node kind, not of derivations, so a word-boundary pass yields
`MemoId` naming effect and cell ids — *"strictly worse than today, **and it
compiles**, so nothing catches it."* Its prescribed fix is `SlotId` → `NodeId`.

`NodeId` already exists (`src/distributed.rs:38`, re-exported at
`src/lib.rs:178`). **It is not a peer id** — an earlier draft of this section
said so and was wrong. It is documented as the *"stable, transport-facing
identifier for a reactive node (cell or slot)… deliberately decoupled from the
crate-internal `SlotId` so the on-the-wire identity stays stable independent of
internal allocation details."*

So the two are the **same concept at two stability tiers**, not a collision
between unrelated things:

| | scope | stability |
|---|---|---|
| `SlotId` | crate-internal | allocation-dependent, recycled |
| `distributed::NodeId` | wire-facing | stable by design |

**Name the tier, not the graph.** `SlotId` → `NodeId` (internal);
`distributed::NodeId` → `WireNodeId`, or keep it module-scoped and stop
flat-exporting it. This beats `GraphNodeId`, which disambiguates a collision that
is not really one while leaving the actual distinction unnamed.

**`CellId` is wrong for the internal id.** It covers effects
(`pending_effects: VecDeque<SlotId>`, `all_effect_ids: Vec<SlotId>`,
`run_effect(id: SlotId)`), and an `Effect` is not a `Cell` — that is the §1
boundary. `CellId` would name effect ids as cell ids: the carve-out's own error
with a different word.

**Open — could the *wire* id be `CellId`?** Its doc says "cell or slot" and omits
effects, which would make `CellId` accurate there. But `EdgeBody` carries
`dependent`/`dependency` as `NodeId`s and an effect *is* a dependent, so mirrored
effect edges would put effect ids on the wire. `NodeState`'s `Opaque` variant
("visible but non-serializable") may be how an effect appears — or may be a cell
holding a non-serializable value. **Verify before renaming the wire type.**

Re-verify per binding; any binding with a distributed plane has both tiers.

### 5.2 `MapHandle` probably collapses

`ReactiveMap<K, V, H>` abstracts over `CellHandle` / `SlotHandle` through the
`MapHandle<V>` trait, whose payload is `const KIND: EntryKind`
(`src/cell_family.rs:94-97`). Under `Cell<T, K>` the kind **is** the type
parameter, so `MapHandle` is expressing something the type system now expresses
directly. Likely deletable, or reducible to a bound on `K`. Not scoped here —
flagged so it is not missed.

### 5.3 `Source<M>` marker vs `Source<T>` trait coexist during migration

`Source<T>` is currently a trait (`merge.rs:289`) and becomes a kind marker.
Reuse is fine once the trait is gone, but there is a window where both exist with
different meanings. Sequence so that window never opens: introduce the marker
under a temporary name, or delete the trait first.

### 5.4 Merge exists on only one execution model

`merge_cell` / `apply_merge` are on `Context` alone (`src/context.rs:1192,1205`).
`ThreadSafeContext` and `AsyncContext` have **zero** merge API. The reactive-graph
corpus replays against all three, so any fixture exercising merge either needs
library work on two models or a capability-aware divergence path. See §8.

### 5.5 "Dependent" points the wrong way

Recorded because the superseded plan's §6 proposes `Dependent` as the trait for
nodes with **incoming** edges, while this codebase's `dependent_count` is
documented as *"how many nodes currently depend on `node` — the size of its
**reverse** edge set"* (`src/reactive_graph.rs:137-139`). "Dependent" already
means **downstream** here.

`DependentCell` would therefore read as "a cell that has dependents," which is
true of every cell. This is why the partition is named `Source`/`Formula` and not
`Source`/`Dependent`.

**Process note, worth institutionalizing alongside §3's ecosystem audit: audit a
candidate against the codebase's own API before the ecosystem's.** `Slot` died to
an external collision; `Dependent` would have shipped with an internal one.

---

## 6. Naming — candidates and why they lost

| candidate | why not |
|---|---|
| `Memo` | names a *behaviour* (caching), not a thing; reads wrong on a wire whose variants are all things. Also collides with `memo()`, which selects the equality guard, while **both** constructors cache |
| `Computed` | ecosystem-wide it means a derived *value*, and TC39 owns it as a class name |
| `Derived` | C++/Java "base and derived classes" — in `lazily-cpp`, reads as *subclass* |
| `DeriveCell` | verb+noun against `SourceCell`'s noun+noun; and `derive` is Rust's most common attribute, appearing thousands of times in the same files |
| `Derivation` | correct noun, and the superseded plan reaches for it unprompted (§1 labels its rows "derivation type"/"derivation constructor") — but a kind marker wants a modifier, and `Cell<T, Derivation>` reads as a category error |
| `DependentCell` | "dependent" means downstream in this codebase — see §5.5 |
| `Latch` | C++20 `std::latch`, Java `CountDownLatch`, the family's own "disposers latch"; also the wrong axis — a latch *stores* |

**`Formula` wins on the plan's own argument.** §3 justifies the direction by
saying `Cell` + the derived type should read as a spreadsheet. A spreadsheet cell
holds either a literal or a **formula**. `FormulaCell` makes the metaphor literal
rather than an analogy in a design doc, reads immediately to someone who has
never seen a reactive graph, and has no collision in any of the eight bindings.

Its one weakness, stated honestly: it is metaphor-bound. If the spreadsheet
framing ever stops being how the library is explained, `Formula` is stranded
where `Derivation` would still be descriptive.

---

## 7. What carries forward unchanged

These sections of the superseded plan are **not** invalidated and should be
lifted verbatim rather than rewritten:

| section | why it survives |
|---|---|
| §5 — the partition; three DAG positions; source-side collapse | this design *is* §5 taken seriously; `Cell ≡ MergeCell<KeepLatest>` becomes a type alias |
| §7 — `Effect` → `MergeCell`; delivery per settled cone; synchronous fold in a batch | unaffected by naming; still normative and still under-fixtured |
| §8 — feedback loop analysis; three termination classes; `KeepLatest` undecidable | mathematics, not policy |
| §8.1 — the effect body is the only yield point | verified against the sync drain |
| §9.5 — verification discipline | mutation-verify; print fixture counts; weaken nothing |
| §10b — settled/open/declined ledger for feedback | including the declined `fixpoint` and `FeedbackEffect` |

§6's `Dependent` trait carries forward **as a concept** — the seam for nodes with
incoming edges, still internal — but **not under that name**, per §5.5.

---

## 8. What has already landed

Independent of naming; survives this redesign intact.

| repo | commit | what |
|---|---|---|
| `lazily-rs` | `f65676c` | Bounded effect drain + `DrainExhaustion` with per-effect attribution. §10b's "build this first". Mutation-verified: with the bound removed the divergent test hangs (SIGTERM at 15s, zero result lines) |
| `lazily-spec` | `845283a` | Allowlist widened for `merge_cell` / `merge` / `merges_of` / `drain_exhausted`. Permits without requiring |
| `lazily-spec` | `10a01da` | Five fixtures on branch **`pending/mergefeed-fixtures`**, deliberately not on `main` |

**Do not land the parked fixtures early.** Runners assert their on-disk fixture
set equals a hardcoded `FIXTURES` list exactly
(`lazily-rs/tests/reactive_graph_conformance.rs:38-40,61`), *and* panic on
unknown ops (`tests/reactive_graph/engine.rs:336`). Either mechanism turns all
eight corpora red the moment the files appear. Runners accept first; fixtures
emit second.

---

## 9. Open items

Deliberately not decided. None blocks drafting; each blocks some part of
execution.

| question | trigger / owner |
|---|---|
| ~~**Go**: interface or runtime rejection?~~ | **DECIDED — interface, see §4** |
| ~~**Merge coverage**: one execution model or three?~~ | **DECIDED — three, implemented not ledgered. See §9.1** |
| Does `MapHandle` collapse entirely, or become a bound on `K`? | during the `lazily-rs` port (§5.2) |
| ~~Could the **wire** id be `CellId`?~~ | **MOOT — `SlotId`/`SlotValue` stay (§5.0). A slot holds any node kind, so the occupant question never arises** |
| **Re-count the reactive-value sense of `slot` per repo** | before scheduling; the old ~12,000 is now an upper bound (§11) |
| A `feed_async(cell, producer)` convenience constructor? | **trigger: two real call sites** (§9.2) |
| ~~Async fold timing~~ | **RESOLVED — cells are the synchronous input layer, so merge is synchronous in `AsyncContext` too. Was overstated. See §9.1** |
| Does the async scheduler coalesce N invalidations in a batch into one effect rerun? | **now tested by fixture 1** rather than assumed (§9.1) |
| Wire vocabulary: does `SlotValue` become `CellValue`? | the wire names things, and `Cell` is a thing — so this is now coherent in a way `MemoValue` never was. Still a breaking wire change across 8 bindings **plus `agent-doc`'s IPC**; dual-accept applies |

---

### 9.1 Merge coverage — DECIDED: implement on all three, do not ledger

The fixtures did not create this work; they **found a gap**. `merge` is the
algebra under RelayCell, whose transports are explicitly `InProc`,
**`CrossThread`**, `Ipc`, `Ws` (`src/relay_transport.rs:6`). Cross-thread is a
first-class use case for the merge plane, and yet `ThreadSafeContext` has no
merge API at all. That reads as an omission nobody noticed rather than a scope
decision.

**Ledgering is the wrong instrument.** The ledger's own contract is that entries
are *findings against the implementation* — "a new divergence fails the build and
a fixed one fails it until the entry is removed." That is built for defects,
which are meant to get fixed. A permanent capability gap parked there would
corrode it, and a ledger with permanent entries stops being read.

It would also leave §7's *"a `Source` never acquires a dependency edge"* — a
structural invariant of the design, not a per-model behaviour — verified only
where it is least likely to be violated. The thread-safe model is exactly where
an implementer feels the pull toward a direct edge, because routing a feed
through a scheduled effect *feels* racy even though it is not.

**All four fixtures apply to all three runtimes.** No per-model applicability is
needed, and none should be built.

| model | action | why |
|---|---|---|
| `ThreadSafeContext` | **implement** | mutex-backed, same semantics; `apply_merge` reads untracked, folds, routes through `set_cell` — should be a straight port, **unmeasured, verify before calling it mechanical** |
| `AsyncContext` | **implement** | not a spec extension — see below |

**The async question was overstated in an earlier draft**, which asked what
"folds synchronously" means when the upstream resolves asynchronously. That
presumed merge is async in `AsyncContext`. It is not:

> *"Cells are the synchronous input layer: `cell`, `get_cell`, and `set_cell`
> are synchronous. Only computed slots, memos, and effects are async."*
> — `lazily-spec/docs/async.md:60-62`

`apply_merge` is **entirely** cell operations — read untracked, fold, route
through `set_cell`. So §7's "merge folds synchronously inside a batch" holds
verbatim in `AsyncContext`, not by analogy. `batch` is likewise *"a
**synchronous** boundary"* there. Fixture 2 therefore passes for exactly the same
reason it passes in `Context`.

The only difference is **when the feeding effect runs**: *"async reruns execute
after the batch returns, on the runtime executor"* (`async.md:210-214`). That is
timing, not semantics, and `GraphModel::settle()` already exists for it — it
*"changes when the corpus's assertions are evaluated, never what they assert."*

**What this makes fixture 1 into: a discriminating test for the async scheduler.**
It asserts one merge per settled cone, which in `Context` holds because effects
are scheduled rather than inline, so N invalidations coalesce into one run. The
async docs claim the same coalescing — *"schedules **an** async rerun after the
current invalidation pass"*, and *"one in-flight computation per revision"* — but
**claimed is not fixtured**, which is the exact state signal eagerness sat in
while a binding shipped a per-invalidation puller under a green checkmark. So the
fixture now tests that claim rather than assuming it.

**Known coverage gap, additive not blocking.** The four fixtures have the feed
read a plain *cell*, and cells are synchronous — so as written they never
exercise the async-specific path. A fifth fixture whose feed reads an **async
formula**, where the effect cannot run until that formula resolves, is what
actually stresses it. Add it separately rather than complicating the four.

Sequence: implement merge on `ThreadSafeContext` and `AsyncContext` → land all
four fixtures against all three models → add the async-formula feed fixture.

---

## 9.2 Async and threaded execution — where the async is allowed to live

**Question:** a `MergeCell` may be computationally expensive or involve a network
call. Can cells support async or threaded execution?

**Yes — but the async lives in the producer, never in the fold.**

```rust
pub trait MergePolicy<T> {
    fn merge(old: &T, op: T) -> T;   // MUST be associative
    const COMMUTATIVE: bool;
    const IDEMPOTENT: bool;
    const CONFLATES: bool;
}
```

The algebra **is** the convergence guarantee, so the fold stays synchronous. An
async fold breaks four specific things, not merely "reasoning" in the abstract:

1. **Lost updates.** `apply_merge` reads old, folds, routes through `set_cell`. An
   await in that window lets another write land. The fix is CAS-and-retry, or
   holding a lock across an await — a deadlock hazard in `ThreadSafeContext`.
2. **Associativity stops being checkable.** The law assumes each `⊕` is atomic.
   With suspendable folds two can be in flight against one cell, so "the current
   value" is ambiguous and `merge_laws.rs` would be property-testing a law that
   no longer describes the runtime.
3. **`CONFLATES` / relay coalescing.** Conflating N pending ops means folding
   them; async folds make coalescing an async batch with its own ordering
   questions — inside the backpressure path.
4. **Batch semantics.** §7's "N calls → N folds, one invalidation" cannot be
   guaranteed at batch exit with folds still pending.

Plus lazily-formal's convergence proofs assume a synchronous fold.

**The supported shape** — §7's construction, with the awaiting done by the
effect:

```rust
effect_async(|ctx| async {
    let v = fetch_remote().await;   // expensive / network — async, outside the algebra
    acc.merge(ctx, v);              // fold — synchronous, cheap, associative
})
```

So: *the algebra is cheap and pure; the work that produces ops is arbitrary and
may be async.* A `MergeCell` fed from a network call is already expressible.

**Encapsulation, and the trade.** Putting the I/O inside the policy would
encapsulate the logic — one call site instead of an effect plus a fetch plus a
merge. That benefit is real, and it is the honest argument for async folds. But
it is available without them: encapsulate the **feeding construction** rather
than the fold.

```rust
ctx.feed_async(acc, || async { fetch_remote().await });
```

Same encapsulation, none of the four costs. **Do not build it yet** — same
grounds §10b of the superseded plan declined `feedback(upstream, target, f,
done)`: designing ergonomics before anyone has written the pattern is guessing.
**Trigger: two real call sites.**

### 9.2.1 Constraint to add — merge policies must be cheap

Nothing in `MergePolicy`'s docs says the fold must be cheap or non-blocking, and
in `ThreadSafeContext` **the fold runs under the mutex**. An expensive `merge`
therefore holds the lock for its duration, and nothing warns the author.

Add a normative constraint: `MergePolicy::merge` **MUST NOT** block and **SHOULD**
be O(1)-ish in the size of `old`. Unenforceable by the corpus — it constrains
implementations, not observable behaviour — so it is review-enforced, the same
construction §7 uses for its documentation obligation.

---

## 10. Non-goals

- **`Cell` → anything.** It survives the spreadsheet-genus objection and is now
  the genus in fact.
- **Making the incoming-edge seam public.** Callers hold a `SourceCell`, a
  `FormulaCell`, or an `Effect` and never need to say "either."
- **`Source` → `Input`.** `Source` is correct in two senses at once — writable,
  and graph-theoretically a node with no incoming edges. `Input` collides with
  the Python builtin, the HTML element, and `InputStream`.
- **The guarded/unguarded constructor split.** Today `computed()` is unguarded
  and `memo()` guarded. Under one type this is a construction flag, not two type
  names — but *which* is the default is an API question, judged on its own and
  not with a migration riding on it.
- **`ctx.pull(formula)` returning a disposable.** The most thesis-consistent
  shape for eagerness. Separate decision.

---

## 11. Risk

**The volume is large but materially smaller than the superseded plan's ~12,000.**
That figure counted *every* word-boundary `slot` site across nine repos. Under
§5.0 the storage sense **stays**, so every `SlotId`, `SlotIdSet`, `SlotIdMap`,
`SlotIdHashBuilder`, `SlotValue`, and slab-vocabulary occurrence is untouched —
including the 87 `SlotId` uses in `context.rs` alone, and the whole wire surface.

**Re-count before scheduling.** The remaining churn is the *reactive-value* sense
only, and nobody has measured that separately from the storage sense. The old
per-repo table (`lazily-rs` 8,408; `lazily-py` 1,116; `lazily-zig` 851;
`lazily-dart` 530; `lazily-go` 378; `lazily-kt` 370; `lazily-js` 276;
`lazily-cpp` 275) is now an **upper bound, not an estimate**.

Density in `lazily-rs` was `context.rs` (174), `thread_safe.rs` (168),
`spec_compliance.rs` (161), `async_context.rs` (143), plus `benches/` — and
`#lzbenchver` gates on benchmark drift, so benches are in scope.

**But the churn now buys a concept removed rather than a word swapped.** That is
the difference from the superseded plan, and the only honest justification for
touching that many sites: nine names out, three in, and `Reactive<T>` — a trait
with four impls and zero uses — deleted rather than renamed.

**The real risk remains scope creep**, exactly as the superseded plan's §13 said.
Every item in §10 is more interesting than the kernel change, and none belongs in
it.

**Pre-existing, unrelated:** `lazily-rs` `make check` is red on `spec_compliance`'s
`#lzbenchver` version-drift gate — `BENCHMARKS.md` does not name 0.46.0. Confirmed
on a clean tree.
