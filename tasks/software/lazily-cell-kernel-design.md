# Design: the Cell kernel — `Source` / `Computed` handles, `Cell` the value-node, and the names that go away

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
Source<T, M>    handle you hold — writes from outside; folds op under policy M   → SourceCell node
Computed<T>     handle you hold — computed from upstream; guarded by default     → ComputedCell node
Effect          no value; sink — outside the hierarchy

Cell            the value-bearing-node *concept* (a SourceCell or a ComputedCell); never a handle
```

**The layering.** You hold a **handle** — `Source<T, M>` or `Computed<T>`, a
lightweight copyable id carrying a `SlotId`. The `SlotId` addresses a **slot** (an
arena position); the slot holds a **node** — the data (`Node::Source(SourceNode)`
or `Node::Computed(ComputedNode)`), with the readable value. `Cell` is the word for
that value-bearing node: a `SourceCell` / `ComputedCell` *is* a node with a readable
value, and the handle *denotes* it. Two concrete handle structs, no `Cell<T,K>`
genus and no phantom kind param — `Source` has `get`/`set`/`merge`, `Computed` has
`get` only, so `computed.set(…)` is a compile error (§3's write-protection without
a trait).

> **History (v1 → v2, 2026-07-21).** v1 shipped `Cell<T,K>` as the *handle* with
> `SourceCell`/`FormulaCell` aliases — which overloaded `Cell` (a handle reading
> like a value) and split the handle names from the internal `Node::Cell`/`Node::Slot`
> data. v2 drops `Cell` from the handles, names the derived kind **`Computed`**
> (ecosystem-standard — TC39 `Signal.Computed`, Vue/Angular/Solid; these values are
> *dynamically derived from reactive state*, `computed`'s sense, not `formula`'s
> static-equation sense), and renames the internal node data to match. `SlotId` /
> arena slot / wire `SlotValue` unchanged (storage, §5.0); the `Source<M>` kind
> marker folds into `Source`'s policy param; the read-genus dissolves (§2:
> bound-needed nowhere). **All cells are guarded — always, no unguarded mode**
> (DECIDED 2026-07-21): `Source` suppresses an equal write (the `==` store-guard,
> already true), `Computed` an equal recompute (matching TC39 `Signal.Computed`),
> with `T: PartialEq` the *uniform* cell bound (this is what makes a guarded
> `computed` express­ible — `PartialEq` is universal, not a special case). `memo`
> is **removed** (redundant); `computed` is the sole guarded derived constructor.
> There is no unguarded escape: if the guard suppresses an update you wanted, the
> *value* under-specified the change — make it `PartialEq`-distinct, or use a merge
> policy. `Reactive` survives only as the umbrella adjective (reactive graph =
> cells + effects; `SyncReactiveGraph` kept), never a type or one-kind synonym.
> Full scheme: `scratchpad/naming-spec-v2.md`. §3 / §5.0 / §1.1 below are v2.

Both kinds answer the **same question**, which is why the pair works:
[`lazily-memo-rename-plan.md`](lazily-memo-rename-plan.md) §5.1 states the
partition as *"a node's value comes either from outside it or from upstream of
it."* `Source` — from outside. `Computed` — from upstream. Same axis, both sides,
no leftover.

`Effect` stays outside and cannot be folded in: it has no value to read, so
nothing can ever depend on it. That is a real boundary, not a naming accident —
the sink position is a capability, not a current degree.

### 1.1 Name ledger

*(v2, 2026-07-21 — supersedes the v1 ledger; see §1 History.)*

| deleted | replaced by |
|---|---|
| `Reactive<T>` (read trait) | gone — the read genus is a bound needed nowhere (§2); handles are concrete |
| `Source<T>` (write trait) | gone — writing is `Source<T, M>`'s inherent `set`/`merge` |
| `MergeCell` | `Source<T, M>` with `M ≠ KeepLatest` |
| `Slot` *(as a reactive value)* | `Computed<T>` (handle) / `ComputedCell` (node) |
| `Signal` | `Computed<T>` + puller `Effect` (`computed().eager()`) |
| `SlotHandle<T>` | `Computed<T>` |
| `SignalHandle<T>` | `Computed<T>` + effect |
| `MergeCellHandle<T, M>` | `Source<T, M>` |
| `CellHandle<T>` | `Source<T>` (= `Source<T, KeepLatest>`) |
| `Cell<T, K>` *(v1 genus handle)* | gone — two concrete handle structs, no genus |
| `Node::Cell(CellNode)` *(internal)* | `Node::Source(SourceNode)` |
| `Node::Slot(SlotNode)` *(internal)* | `Node::Computed(ComputedNode)` |

Added: `Source<T, M>` (handle), `Computed<T>` (handle), `SourceCell` / `ComputedCell`
(the two *cell* kinds — the node concept), `Node::Source` / `Node::Computed` (data).

**`Cell` is not a handle at all** — it is the word for a value-bearing node (a
`SourceCell` or a `ComputedCell`). **`Slot` stays as storage** (§5.0): `SlotId`,
`SlotValue`, arena position, slab vocabulary all stay. What v1 wrongly left as the
*derived-value* sense (`Node::Slot`) becomes `Node::Computed`, so "Slot = position"
is finally true throughout.

**Constructors** (§9.3): `source` / `computed` / `.eager()` replace `cell`,
`merge_cell`, `memo`, `slot`, `signal`, `get_signal`, `dispose_signal`. `computed`
is **guarded by default** (unlike its historical unguarded sense in these
bindings). The eager construction is `computed().eager()` — idempotent, returning
the same handle — which retires `Signal`, `SignalHandle`, and makes the
`#lzsignaleager` clause-3 bug structurally unwritable.

`Cell ≡ MergeCell<KeepLatest>` stops being a spec assertion and becomes a type
alias with a default parameter.

---

## 2. `Reactive<T>` is already vestigial — measured

The argument for deleting it is not aesthetic. **`Reactive<T>` is implemented for
four handle types and used as a generic bound nowhere.**

*(Measuring the pre-v2 `lazily-rs` code — the four impls named here are the v1
handle types the trait unites; v2 collapses them into `Source` / `Computed`, so
the trait's job disappears with them.)* All 13 occurrences in `lazily-rs`: the
trait definition (`merge.rs:263`), `Source<T>`'s supertrait clause (`:289`), four
impls (`CellHandle` `:297`, `SlotHandle` `:313`, `SignalHandle` `:319`,
`MergeCellHandle` `:325`), and doc comments. `rateshape.rs` says "over any
`Reactive<T>` source" three times in prose and **does not take the bound** in any
signature.

So the trait's entire job today is asserting *"these four types read alike."*
Collapse them into the two concrete v2 handles and the assertion is vacuous.
`Reactive<T>` is a consequence of having four handle types, not a reason to keep
them. Under v2 it survives only as an umbrella adjective (§1.1), never a trait
uniting the kinds.

**No trait-object need.** `grep 'dyn Reactive'` returns nothing. Where the
codebase needs heterogeneity it uses an enum, not `dyn` — the conformance
runner's `Ref<G>` is exactly that. The one thing a generic type could not serve
turns out not to be required.

---

## 3. Mechanism: write protection without a trait

*(v2: two concrete handle structs, no genus.)* The kind is the struct itself, and
the write methods live only on `Source`:

```rust
pub struct Source<T, M = KeepLatest> { id: SlotId, _m: PhantomData<(T, M)> }
pub struct Computed<T>               { id: SlotId, _t: PhantomData<T> }

impl<T, M: MergePolicy<T>> Source<T, M> {
    pub fn get(&self, ctx: &Context) -> T { … }
    pub fn set(&self, ctx: &Context, v: T) { … }         // Source writes
    pub fn merge(&self, ctx: &Context, op: T) { … }
}

impl<T> Computed<T> {
    pub fn get(&self, ctx: &Context) -> T { … }          // read only
    pub fn eager(self, ctx: &Context) -> Computed<T> { … }
}
```

`computed.set(…)` fails with *"no method found"* — a compile error with no trait
and no genus in sight, because `Computed` simply has no `set`. The policy `M` lives
**inside** `Source`, so it exists exactly where writes exist and is absent on the
computed side. Both handles denote a *cell* — a `SourceCell` / `ComputedCell` node
in the arena — but `Cell` itself is never a handle struct.

There is **no read-genus type**. §2 shows a "read any kind" bound is needed
nowhere; where heterogeneity is genuinely required the codebase uses an enum (the
conformance runner's `Ref<G>`), not a genus. Dropping the v1 `Cell<T, K>` genus
removes both the phantom kind param and the `Cell`-reads-like-a-value overload.

---

## 4. Portability — v2 ports everywhere (the v1 exception dissolves)

The load-bearing question is whether `computed.set(…)` can be made a *compile
error*, and under v2 the answer is **yes in every statically-typed binding,
including Go** — because the mechanism is now **which concrete handle carries the
write methods** —
`set`/`merge` live on `Source`, never on `Computed`. **This is a trait/interface
hierarchy question, not a class inheritance one** — five of eight bindings have no
inheritance at all.

The v1 design pursued this differently — by restricting a single `Cell<T, K>`
genus's methods to the `Cell<T, Source<M>>` instantiation — and that
type-parameter restriction is what did not port everywhere. The table below is
that v1-mechanism portability record; v2 sidesteps it entirely by dropping the
genus (two concrete handles, writes on `Source`):

| binding | can restrict a genus's methods by type param (v1) | note |
|---|---|---|
| rs | ✅ | inherent impls on the specific instantiation → v2: inherent impls on the `Source` handle |
| zig | ✅ | comptime |
| cpp | ✅ | concepts / partial specialization |
| kt | ✅ | extension functions declared on `Cell<T, Source<M>>` → v2: on `Source<T, M>` |
| **go** | ❌ | **generics cannot restrict methods by type parameter, and methods cannot introduce their own** — which is exactly why v2's two-concrete-handle shape is the portable one |
| py / dart / js | n/a | no compile-time enforcement either way |

**DECIDED 2026-07-20 — Go keeps the compile error via distinct structs.**
Runtime rejection was rejected: the whole point of putting the kind in the type
is that `computed.set()` fails to build, and a binding that downgrades that to a
panic gives up the guarantee while paying all the migration cost.

Under v2 there is no genus to reintroduce — go writes the two handles as distinct
structs, and `Computed` simply omits `Set`, so the compile error survives without
any genus:

```go
type Source[T any, M MergePolicy[T]] struct{ … }  // has Set/Merge
type Computed[T any]                  struct{ … }  // has neither
```

`computed.Set(…)` is undefined at compile time. Because v2 dissolves the genus for
every binding (§Genus / §3), go is no longer reintroducing anything the others
delete — it holds two concrete handles exactly as `lazily-rs` does. Go MAY keep a
small `type Cell[T any] interface { Get(ctx *Context) T }` read abstraction, but
only where its own generic code genuinely needs to read either kind — an optional
per-binding convenience, not a resurrected genus.

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
kind — `SourceCell`, `ComputedCell`, or an effect node. The container's name does
not depend on its occupant, which is exactly what defeated `CellId` (§5.1) and
`NodeId` (§5.1).

**Consequence: `SlotValue` on the wire is correct, not a seam.** It is the value
in a slot. The superseded plan's §13 dilemma — leave a permanent seam, or take a
breaking wire change across eight bindings **plus `agent-doc`'s IPC** — simply
does not arise. No rename, no dual-accept.

Layering: a `Source` or `Computed` handle carries a `SlotId`; the slot holds a
node. The `recycled_id_inherits_nothing` fixture becomes vividly named — the slot
persists, the occupant changes.

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

`ReactiveMap<K, V, H>` abstracts over the `Source` / `Computed` handles through the
`MapHandle<V>` trait, whose payload is `const KIND: EntryKind`
(`src/cell_family.rs:94-97`). Under v2 the kind **is** the concrete handle struct
(`Source` vs `Computed` are already distinct types), so `MapHandle`'s `const KIND`
is expressing something the type system now carries directly. Likely deletable, or
reducible to a per-handle bound. Not scoped here — flagged so it is not missed.

### 5.3 `Source<M>` marker vs `Source<T>` trait coexist during migration

`Source<T>` is currently a trait (`merge.rs:289`) and the name is reused for the
v2 `Source<T, M>` handle. Reuse is fine once the trait is gone, but there is a
window where both exist with different meanings. Sequence so that window never
opens: introduce the handle under a temporary name, or delete the trait first.

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
true of every cell. This is why the partition is named `Source`/`Computed` and not
`Source`/`Dependent`.

**Process note, worth institutionalizing alongside §3's ecosystem audit: audit a
candidate against the codebase's own API before the ecosystem's.** `Slot` died to
an external collision; `Dependent` would have shipped with an internal one.

---

## 6. Naming — candidates and why they lost

| candidate | why not |
|---|---|
| `Memo` | names a *behaviour* (caching), not a thing; reads wrong on a wire whose variants are all things. Also collides with `memo()`, which selects the equality guard, while **both** constructors cache |
| `Derived` | C++/Java "base and derived classes" — in `lazily-cpp`, reads as *subclass* |
| `DeriveCell` | verb+noun against `SourceCell`'s noun+noun; and `derive` is Rust's most common attribute, appearing thousands of times in the same files |
| `Derivation` | correct noun, and the superseded plan reaches for it unprompted (§1 labels its rows "derivation type"/"derivation constructor") — but a kind marker wants a modifier, and `Cell<T, Derivation>` reads as a category error |
| `DependentCell` | "dependent" means downstream in this codebase — see §5.5 |
| `Latch` | C++20 `std::latch`, Java `CountDownLatch`, the family's own "disposers latch"; also the wrong axis — a latch *stores* |

**`Computed` wins — DECIDED 2026-07-21 (v2), reversing the v1 pick of `Formula`.**
The v1 argument for `Formula` was the spreadsheet metaphor: `Cell` + the derived
type reads as a spreadsheet (a cell holds a literal or a formula). But v2 (§1
History) drops `Cell` from the handles, so the derived handle no longer pairs with
`Cell` — it pairs with `Source` (`Source<T,M>` / `Computed<T>`). That removes
`Formula`'s main support, and the pairing that remains, **`Source` / `Computed`**,
is the one the whole signals ecosystem already uses — TC39 `Signal.State` /
`Signal.Computed`, Vue/Angular/Solid. These values are *dynamically derived from
reactive state* (recomputed when dependencies change), which is exactly
`computed`'s sense and not `formula`'s static-equation sense.

The v1 objection ("ecosystem-wide `Computed` means a derived value, and TC39 owns
it as a class name") was backwards: matching the standard's vocabulary is a
feature, and `Signal.Computed` is namespaced — no real collision. `Formula`'s
honest weaknesses were that it is metaphor-bound and non-standard in a signals
library. Two costs to name explicitly: `Computed` is an adjective used as a handle
noun ("a `Computed`"), which the ecosystem already nominalizes; and `computed(f)`
historically named the *unguarded* constructor in these bindings, so v2 makes
`computed`'s guard-by-default explicit in its docs.

---

## 7. What carries forward unchanged

These sections of the superseded plan are **not** invalidated and should be
lifted verbatim rather than rewritten:

| section | why it survives |
|---|---|
| §5 — the partition; three DAG positions; source-side collapse | this design *is* §5 taken seriously; the plain source `Source<T>` ≡ `Source<T, KeepLatest>` becomes a type alias with a default parameter |
| §7 — `Effect` → merge cell (`Source<T, M>`); delivery per settled cone; synchronous fold in a batch | unaffected by naming; still normative and still under-fixtured |
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
| ~~**Go**: interface or runtime rejection?~~ | **DECIDED — distinct `Source`/`Computed` structs keep the compile error (`Computed` has no `Set`); `Cell[T]` interface optional, see §4** |
| ~~**Merge coverage**: one execution model or three?~~ | **DECIDED — three, implemented not ledgered. See §9.1** |
| Does `MapHandle` collapse entirely, or become a bound on `K`? | during the `lazily-rs` port (§5.2) |
| ~~Could the **wire** id be `CellId`?~~ | **MOOT — `SlotId`/`SlotValue` stay (§5.0). A slot holds any node kind, so the occupant question never arises** |
| **Re-count the reactive-value sense of `slot` per repo** | before scheduling; the old ~12,000 is now an upper bound (§11) |
| A `feed_async(cell, producer)` convenience constructor? | **trigger: two real call sites** (§9.2) |
| Does anything revert eager→lazy? (decides whether `lazy` exists) | measure before building `lazy` (§9.3.4) |
| `computed` guarded by default; `memo` removed | **DECIDED — all cells guarded, always, no unguarded mode; `T: PartialEq` is the uniform bound; behaviour change, migration note (§9.3)** |
| Generation-tagged `SlotId` | proposed — closes the recycled-read hole and makes owner-keyed side tables sound (§9.3.5) |
| Find the driver by id (side table) or by scanning dependents? | during the `lazily-rs` port (§9.3.3–4) |
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
computed**, where the effect cannot run until that computed resolves, is what
actually stresses it. Add it separately rather than complicating the four.

Sequence: implement merge on `ThreadSafeContext` and `AsyncContext` → land all
four fixtures against all three models → add the async-computed feed fixture.

---

## 9.2 Async and threaded execution — where the async is allowed to live

**Question:** a merge cell (`Source<T, M>`, `M ≠ KeepLatest`) may be
computationally expensive or involve a network call. Can cells support async or
threaded execution?

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
may be async.* A merge cell (`Source<T, M>`) fed from a network call is already
expressible.

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

### 9.2.2 A writer is always an Effect — state this once, in the spec

**Question that keeps recurring:** should there be a type encapsulating "set this
cell"? A network-fed cell, a feedback writer, an eager puller?

**No — it is an `Effect`, and this follows from the partition rather than from
convention.** Anything whose job is to write *reads* something and produces no
readable value. That is incoming edges and no value: the sink row of §5.1, and
§1's reason `Effect` sits outside the `Cell` hierarchy. Nothing can depend on it,
because there is nothing to read.

The family has now reached this same answer three times, from three directions:

| construction | proposed as | actually is |
|---|---|---|
| eager values | a node kind (`Signal`) | eager `Computed<T>` + puller `Effect` |
| feedback | a node kind (`FeedbackEffect`) | `Effect` + `merge` |
| writer encapsulation | a node kind / new type | `Effect` + `set` |

**The theorem:** *writing is not having a value, so a writer is always a sink.*
The partition is on what a node's **value** is; a writer has none. Each of the
three arrived as "should this be a new kind?" and each was answered from scratch.
Stating it once in `reactive-graph.md` retires the question.

What remains open is only ever the *ergonomics*, and that is a three-way split
worth keeping distinct:

| | verdict |
|---|---|
| new **node kind** | ✗ — the kernel is closed because the DAG positions are closed |
| new **type / handle** | ✗ for now — `Effect` already provides disposal, the only lifecycle a writer needs. A distinct type earns its place only if it carries state a handle does not (pause, reconfigure the producer, retarget) |
| named **constructor** returning an `Effect` | ✓ on evidence — `feed_async` (§9.2), deferred on **two real call sites** |

### 9.2.3 Arguments vs dependencies — and a one-line definition of feedback

An `Effect` relates to cells in **two** ways that look alike and behave nothing
alike:

| relationship | direction | edge? | which kinds |
|---|---|---|---|
| **reads** a cell | upstream | ✅ dependency, discovered dynamically per run | any cell (`Source` or `Computed`) |
| **writes** a cell | outward | ❌ none | only `Source<T, M>` |

A write target is an **argument** — passed in, captured by the closure, known
statically. A read is a **dependency** — discovered at run time by the tracking
stack and re-discovered on every rerun.

This is why `merge_cell_acquires_no_dependency_edge.json` asserts on degrees
rather than values: a binding modelling the write target as an edge satisfies
every value assertion while creating a node that is both writable and dependent,
which §5.1 says cannot exist.

**The kernel enforces the "Sources only" half at compile time.** `set` and
`merge` live on the inherent impl for `Source<T, M>`, so an `Effect` cannot
take a `Computed<T>` as a write-argument. Not a convention to document — it does
not build.

**Definition to adopt:**

> **Feedback is when an argument is also a dependency.**

The effect writes a cell it also reads. Nothing about that is a graph cycle — the
argument creates no edge — which is exactly why acyclicity checks never fire and
the loop closes through the **scheduler** instead. It also makes §10b's
write-into-own-cone detection trivially statable: *is a write-argument in
`deps(E)`?*

Prefer this phrasing over "an effect that writes into its own dependency cone,"
which describes the same thing while making it sound like a property of the
graph.

### 9.2.1 Constraint to add — merge policies must be cheap

Nothing in `MergePolicy`'s docs says the fold must be cheap or non-blocking, and
in `ThreadSafeContext` **the fold runs under the mutex**. An expensive `merge`
therefore holds the lock for its duration, and nothing warns the author.

Add a normative constraint: `MergePolicy::merge` **MUST NOT** block and **SHOULD**
be O(1)-ish in the size of `old`. Unenforceable by the corpus — it constrains
implementations, not observable behaviour — so it is review-enforced, the same
construction §7 uses for its documentation obligation.

---

## 9.3 The constructor surface, and the eager construction

Three constructors, symmetric with the kernel:

```rust
ctx.source(v)                        // -> Source<T, KeepLatest>; source::<Sum>(v) for other policies
ctx.computed(f)                      // -> Computed<T>  (guarded)
ctx.computed(f).eager()              // -> Computed<T>, now eager
```

`computed` replaces the old unguarded `computed` / `memo` / `slot` — three names
for one thing, now one guarded constructor (**`memo` is removed** — it folds into
the guarded `computed`). It **subsumes `cell` and `merge_cell`**: `source(v)`
defaulting to `KeepLatest` is today's `cell`, `source::<M>(v)` is today's
`merge_cell`. Another two names gone.

**All cells are guarded, always** — DECISION (2026-07-21), and a behaviour change,
not just a rename. `computed` is the sole derived constructor and it is guarded;
there is **no unguarded mode** and no `memo`. `T: PartialEq` is the *uniform* bound
on every cell (source and computed alike), which is what makes a guarded `computed`
expressible without special-casing — the same `==` that already makes an equal
`set` a no-op on `Source` suppresses an equal *recompute* on `Computed` (matching
TC39 `Signal.Computed`). `lazily-react` already documents the guarded form as
*"the default for derived state,"* and the guard is the efficient choice (equal
recomputes do not propagate). Code on the old unguarded `computed()` gains
suppression it did not have — usually an improvement, always a migration note.

**There is no unguarded escape.** If the guard suppresses an update you wanted, the
*value* under-specified the change: either **make the value genuinely
`PartialEq`-distinct** (encode the distinction you care about into the value), or
**use a merge policy** to express accumulate/always-apply semantics. It is not a
library toggle and there is no `equals: false`. (A custom-equality comparator,
still guarded, is a possible future option — not now.)

### 9.3.1 `.eager()` — idempotent, returns the computed

`.eager()` **ensures the computed is eager**: it is a declarative state
transition to eager, not an action that mints a puller. Idempotent — a second
call is a no-op. This matters structurally: without it, `f.eager().eager()`
attaches two pullers to one computed, doubling the eager compute — the same cost
class as the `lazily-go` welded-eagerness bug, from a different direction.
Idempotence makes that unrepresentable.

It returns the **computed**, not a puller handle, so construction cannot be left
half-done and the caller holds the thing it actually reads:

```rust
let f = ctx.computed(|c| …).eager();
ctx.get(&f)                          // ordinary read — no get_signal
```

Note it returns the **same** handle, mutated, not a derived copy — going eager is a
side effect on the graph. Document that explicitly: `let g = f.eager()` gives `g
== f`, both eager; it is not builder-style `with(…)`.

### 9.3.2 This retires the entire `Signal` surface

`Signal` was *"memo slot + puller effect"* (`reactive-graph.md:69`) — a
composition needing its own handle type because it is two nodes. Under
`computed().eager()` the composition is written out, so nothing is left to name:

| retired | replaced by |
|---|---|
| `Signal` (concept) | an eager `Computed<T>` |
| `SignalHandle` | `Computed<T>` — read with ordinary `get` |
| `get_signal`, `is_signal_active` | `get`, and the eager bit |
| `dispose_signal` (API + corpus op) | `lazy`, if it exists (§9.3.4) — else nothing |

**The `#lzsignaleager` clause-3 bug becomes unwritable.** `lazily-go` shipped
`Signal` as a slot subclass overriding `onInvalidate` to re-pull, with no effect
anywhere, correct values at N× compute, every fixture green. Under this design
the eager construction *is* an effect and the coalescing comes from the scheduler
(effects are scheduled, not inline), so the per-write puller cannot be
constructed. A normative clause one binding silently violated for months becomes
a structural property. It also deletes the `dispose_signal` corpus op, so §9.3's
op-rename flag day in the superseded plan never happens.

### 9.3.3 Storing the puller — bit on the node, id in a side table

Associating the puller with its computed must not grow the common (lazy) node.
Split the state:

| state | where | cost |
|---|---|---|
| "am I eager?" | a `bool` on `ComputedNode` | **free** — `ComputedNode` already carries `dirty`/`force_recompute`/`in_progress` (`context.rs:409-414`), three bytes in an 8-byte-aligned padded region; a fourth bool lands in existing padding, the enum does not grow |
| "which effect pulls me?" | `eager_by: HashMap<SlotId, SlotId>` side table | one entry per **eager** computed, zero per lazy one |

This is the `EdgeIndex` precedent exactly (`context.rs:433-434`): per-node state
for the common case, a side table for the rare one, *"kept off the nodes so
low-degree nodes cost nothing."* Eager computeds are rare, so the puller id does
not belong on the node. Storing a `SlotId` (8 bytes) on `ComputedNode` would grow
the largest enum variant, hence every node including lazy ones.

**Safety obligation — the side table is owner-keyed, the `recycled_id` hazard.**
`recycled_id_inherits_nothing` warns that *"any owner-keyed side table will alias
the disposed node's stale index onto the unrelated node that inherits the id."*
So `eager_by` **must** be cleared on computed disposal, and this is why the
generation-tagged `SlotId` (§9.3.5) is not optional decoration — it is what makes
owner-keyed tables like this one sound. The edge index already lives under this
rule; `eager_by` is one more table under it.

The bit alone makes `.eager()` idempotent — no lookup. The side table is needed
only for disposal-tears-down-puller (§9.3.4), and even then only if the puller is
looked up by id rather than found by scanning the computed's dependents for the
flagged one (O(dependents), tiny for a computed).

### 9.3.4 Disposal owns the puller; and the clause-4 check

Disposing an eager computed **must** tear down its puller — otherwise `.eager()`
hands back a computed whose disposal strands a poisoned effect, the regression
this design is meant to remove. The eager bit makes this trivially correct:
dispose computed → if eager, tear down puller → clear the bit and the
`eager_by` entry. The computed always knows whether it has a puller, so no
survivor is stranded.

**Open check that decides one method:** does anything in the family ever revert
an eager value **back** to lazy? If yes, `f.lazy()` exists (the reverse
transition, replacing `dispose_signal`) and the puller is disposable on demand.
If no, `eager` is one-way, there is no `lazy`, and clause 4 was only ever a
description of `Signal` being a pair — an implementation artifact, not a needed
capability. **Measure before building `lazy`.**

### 9.3.5 Generation-tagged `SlotId` — closes a hole in the current handle

A `Computed<T>` handle is `{ id: SlotId, PhantomData<T> }` and `SlotId` is a bare
`u64` (`src/slot.rs:10-12`) — no generation, no epoch. So a stale handle to a
disposed id, after LIFO recycling hands that id to a new node, **looks up the new
occupant and reads it successfully.** The only guard is the `type_id` check,
which catches a stale handle of a *different* type but not same-type recycling —
the likely case, since allocation patterns repeat.

The corpus already forbids the outcome — `read_after_dispose_is_an_error` says a
reader throws *"rather than silently observing a stale, default, or **recycled**
value"* — but nothing in the implementation can detect the recycled case. That is
a normative clause with no structural enforcement, the same shape as the
`#lzsignaleager` gap.

Fix, at no memory cost:

```rust
struct SlotId { index: u32, generation: u32 }   // still 8 bytes
```

Bump the generation when a slot is freed; a lookup compares it. `SlotId` stays
`u64`, handles stay `Copy`, the arena design is untouched. This makes
`read_after_dispose` enforceable in the recycled case, makes every owner-keyed
side table (`eager_by`, the edge index) sound against recycling, and lets any
handle operation report a gone node at lookup rather than as a wrong answer
later. Proposed fixture, unwritable today: *a stale handle to a recycled id must
error, not read the new occupant.*

**Re-audit per binding** — the generation gap and the poisoning semantics may
differ; measure each.

---

## 9.4 Execution — spec and formal first, always

**The plan leads with the spec, then the formal model. No binding churns until
both are green.** This is not generic process hygiene; it is the specific
discipline this design needs, for three reasons the design itself supplies:

1. **The names are the whole risk.** The reason the rename was put on hold is
   that ~12,000 sites cannot churn toward a moving target. The spec is where the
   vocabulary stops moving. Settle the two concrete handles `Source` / `Computed`
   (nodes `SourceCell` / `ComputedCell`), `Slot`-as-storage, `source` / `computed`
   / `.eager()`, and guarded-by-default in prose **first**, and every later step
   targets a fixed vocabulary.
2. **This design's core claims are theorems, not conventions.** The partition
   (§1), "a writer is always a sink" (§9.2.2), and "feedback is when an argument
   is also a dependency" (§9.2.3) belong in the formal model, where they are
   *proved* rather than asserted. If they cannot be expressed, that is a defect
   in the design, and the formal model is the cheapest place on earth to find it
   — before eight bindings, not after.
3. **The formal model is already ahead of the bindings here.**
   `lazily-formal/LazilyFormal/Signal.lean` (29.4 KB, 649+ lines) already proves
   `Signal ≡ Slot.eager` as *"a memo `Slot` plus a puller `Effect`"* — the exact
   composition `.eager()` makes canonical. The bindings shipped *welded*
   eagerness (`#lzsignaleager`); the proof never did. So formal-first is not
   speculative: the model already validates the eager story, and reframing it as
   `computed().eager()` should preserve the proof, which is a thing to *check in
   Lean*, not hope for in Rust.

### Step 1 — Spec prose. No fixture-op or code changes.

Settle the vocabulary and the theorems in `lazily-spec`:

- `docs/reactive-graph.md` — the kernel (the two concrete handles `Source` /
  `Computed`, nodes `SourceCell` / `ComputedCell`), `Slot` redefined as the
  storage position (§5.0), the constructor surface (§9.3), guarded-by-default (all
  cells, `T: PartialEq`, no `memo`), and the three theorems stated once each:
  writer-is-a-sink (§9.2.2), feedback = argument-is-dependency (§9.2.3),
  merge-must-be-cheap (§9.2.1).
- Root `cell-model.md` and `protocol.md` (mdBook `{{#include}}` stubs in `docs/`
  — edit the root, §5.1 correction: no symlink hazard).
- `README.md`, `coverage.json`, `book/`, `examples/`.
- **`SlotValue` on the wire stays** (§5.0) — no wire change, no dual-accept, no
  `agent-doc` IPC break. The §13 dilemma of the superseded plan is void.

Gate: `make check` green. Nothing structural has moved yet.

### Step 2 — Formal. Prove the kernel; catch the inexpressible cheaply.

In `lazily-formal` (and `lazily-spec/formal/lean` where it mirrors):

- `Reactive.lean` (33.4 KB) — the kernel and the partition. Prove every node is
  exactly one of source / computed / effect, and that a writer occupies the sink
  position. This is §1 and §9.2.2 as a proof.
- `Signal.lean` (29.4 KB) — **reframe, do not rewrite.** It already proves the
  slot+effect composition; restate it as `computed().eager()` and confirm the
  materialize-once-per-batch property (clause 3) survives the renaming. That
  property is what a binding got wrong; the proof is what says the composition
  cannot.
- `AsyncSlotState.lean` (6.7 KB) — the async computed FSM; rename in step with the
  vocabulary.
- Generation-tagged `SlotId` (§9.3.5): if the recycled-read property is to be
  *proved* rather than only fixtured, it lives in `Reactive.lean` (not
  `StableId.lean`, which is text identity). Optional at this step; flag if
  deferred.
- **CARVE-OUT: `Signaling.lean` and `SignalingRoster.lean` are WebRTC
  peer-discovery** ("anti-spoof forwarding"), semantically unrelated to reactive
  `Signal`. **Do not touch.** Confirmed by reading their headers.

Gate: `lake build` clean, no `sorry`. If the two-concrete-handle write protection
(writes on `Source`, absent on `Computed`) cannot be modelled, stop and revisit §3
— that is the signal to catch here, at the cost of a Lean file, not a nine-repo
migration.

### Step 3 onward — the mechanical fan-out, unchanged in shape

Only after steps 1–2 are green:

3. Conformance runners **dual-accept** any renamed ops before any fixture emits
   them (runners panic on unknown ops and assert the on-disk set equals
   `FIXTURES` exactly — §8). Land the parked `pending/mergefeed-fixtures`
   (§8) once the merge ops exist in the runners.
4. Reference binding `lazily-rs` — the kernel, `source`/`computed`/`.eager()`, the
   eager bit + `eager_by` side table (§9.3.3), generation `SlotId` (§9.3.5),
   `SlotId`→internal / `distributed::NodeId`→`WireNodeId` (§5.1). Include
   `benches/` — `#lzbenchver` gates on drift.
5. `lazily-js`, honouring the `slot()`/`disposeSlot()` deprecation.
6. `lazily-react` — depends on 5; `useSlot` deleted (zero call sites).
7. Remaining six in parallel — **one agent per repo, never two**. Each replays
   the full corpus and prints counts as proof it ran.
8. `lazily-go` last among them — interface for the write-protection (§4), and it
   is already blocked on a consuming cascade, so do not stack two migrations.
9. Blog (superseded plan §11 survives), superproject pointers, deploy.

**The already-landed work stands and does not re-order:** the drain bound
(`lazily-rs` f65676c) and the merge allowlist (`lazily-spec` 845283a) are
decision-independent infrastructure (Tracks B/C), valid under any naming. They do
not depend on steps 1–2 and are not invalidated by them.

---

## 10. Non-goals

- **`Cell` → anything.** It survives as the value-bearing-node *concept* (§1) —
  the word for a `SourceCell` or a `ComputedCell`, never a handle — and stays.
- **Making the incoming-edge seam public.** Callers hold a `Source`, a
  `Computed`, or an `Effect` and never need to say "either."
- **`Source` → `Input`.** `Source` is correct in two senses at once — writable,
  and graph-theoretically a node with no incoming edges. `Input` collides with
  the Python builtin, the HTML element, and `InputStream`.
- **A guarded/unguarded constructor split.** DECIDED against (§9.3): all cells are
  guarded, always, `T: PartialEq` the uniform bound, `memo` removed, and there is
  no unguarded mode. What an earlier draft left as an open default is settled — the
  escape for "always propagate" is a `PartialEq`-distinct value or a merge policy,
  not a library toggle.
- **`ctx.pull(computed)` returning a disposable.** The most thesis-consistent
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
