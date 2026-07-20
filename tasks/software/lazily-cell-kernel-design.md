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
| `Slot` | `FormulaCell<T>` |
| `Signal` | `FormulaCell<T>` + puller `Effect` |
| `SlotHandle<T>` | `Cell<T, Formula>` |
| `SignalHandle<T>` | `Cell<T, Formula>` + effect |
| `MergeCellHandle<T, M>` | `Cell<T, Source<M>>` |
| `CellHandle<T>` | `Cell<T, Source<KeepLatest>>` |

Added: `FormulaCell`, `Formula`, `Cell<T, K>`. **Nine names out, three in.**

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

**The Go carve-out is the one real casualty.** Go needs either separate types
plus an interface — reintroducing `Reactive<T>` in exactly one binding — or
runtime rejection of writes to formula cells. Decide deliberately; do not let it
be discovered during the port. Note `lazily-go` is already flagged as blocked on
a consuming cascade (`#lzsignaleager`), so it is the binding least able to absorb
an unplanned divergence.

---

## 5. Measured carve-outs

Findings from executing the superseded plan. Each one compiles, or would have,
which is why none of them would surface on their own.

### 5.1 `NodeId` is already taken — the rename plan's own fix collides

`lazily-memo-rename-plan.md` §10 step 6 carves out `SlotId`, correctly: it is the
id of **every** node kind, not of derivations, so a word-boundary pass yields
`MemoId` naming effect and cell ids. Its prescribed fix is `SlotId` → `NodeId`.

**`NodeId` already exists in `lazily-rs`** — `distributed::NodeId(pub u64)`
(`src/distributed.rs:38`), a distributed-systems **peer** identifier, publicly
re-exported at crate root beside `PeerId` (`src/lib.rs:178`). It means *cluster
node*, not *graph node*.

So the carve-out's fix would collide with live public API. Use **`GraphNodeId`**,
or keep both module-scoped and never flat-export both. Re-verify per binding —
any binding with a distributed plane likely has the same clash.

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
| **Go**: interface, or runtime write rejection? | blocks the `lazily-go` port; decide before it starts (§4) |
| **Merge coverage: one execution model or three?** | blocks Track B. Ledgering means §7 is pinned only on the single-threaded model — and the thread-safe model is where a feed-as-an-edge implementation is most tempting (§5.4) |
| Does `MapHandle` collapse entirely, or become a bound on `K`? | during the `lazily-rs` port (§5.2) |
| `GraphNodeId`, or module-scoped `NodeId`? | before the id carve-out (§5.1) |
| Wire vocabulary: does `SlotValue` become `CellValue`? | the wire names things, and `Cell` is a thing — so this is now coherent in a way `MemoValue` never was. Still a breaking wire change across 8 bindings **plus `agent-doc`'s IPC**; dual-accept applies |

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

**The volume is unchanged and large.** ~12,000 word-boundary `slot` sites across
nine repos, `lazily-rs` alone at 8,408 with density in `context.rs` (174),
`thread_safe.rs` (168), `spec_compliance.rs` (161), `async_context.rs` (143),
plus `benches/` — and `#lzbenchver` gates on benchmark drift.

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
