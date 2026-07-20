# Plan: retire `Slot` for `Memo`, add `Dependent`

**Status:** designed 2026-07-20, not started. Supersedes §2 of
[`lazily-signal-followups.md`](lazily-signal-followups.md).

Two renames and one new internal trait. **No semantic change** — but not a pure
substitution: two repos ship `slot` as public API (§1), one wire variant is still
named `SlotValue` (§13), and the affected surface is ~12,000 sites across nine
repos. The trait is a seam the bindings have already grown independently.

**Revised 2026-07-20 after review.** An earlier draft claimed "no wire change, no
API shape change" and rated the risk low. All three were wrong; §1, §10 and §13
carry the corrections.

---

## 1. What changes

| | before | after |
|---|---|---|
| derivation type | `Slot` | **`Memo`** |
| derivation constructor | `ctx.memo(f)` | **unchanged** |
| eager derivation | `Signal` / `ctx.signal(f)` | **`ctx.eager_memo(f)`** |
| handle | `SlotHandle` | `MemoHandle` |
| keyed map | `SlotMap` | `MemoMap` |
| spec notation | `Signal ≡ Slot.eager` | `Memo.eager` |
| internal trait | *(none)* | **`Dependent`** |
| wire field | already `node` / `NodeId` | **unchanged** |
| `Reactive<T>`, `Source<T>`, `Cell`, `MergeCell`, `Effect` | | **unchanged** |

**The constructor does not move.** Today `ctx.memo(f)` returns a `Slot` — the
type/constructor mismatch that started this. The type moves to meet the
constructor, not the reverse.

**But this is NOT a pure internal rename, and an earlier draft of this plan said
it was.** `lazily-js` ships three constructor aliases publicly
(`src/reactive.d.ts:71-73`): `computed()`, `slot()`, and `memo()`, all returning
`SlotHandle` — plus `disposeSlot()` (`:91`). So `ctx.slot()` and `disposeSlot()`
are **published npm API**, and removing them is a breaking change requiring a
deprecation cycle, not a substitution.

| repo | exposure | ordering |
|---|---|---|
| `lazily-js` | **public** `slot()`, `disposeSlot()`, `SlotHandle` in `.d.ts` | deprecate, don't delete |
| `lazily-react` | **public** `useSlot` hook; imports `SlotHandle` across the package boundary | **must follow `lazily-js`** |
| other six bindings | internal type names | independent |
| `lazily-html` | 49 hits are the **HTML `<slot>` element** | **out of scope — do not touch** |

So it is **nine repos, not eight**, with one hard ordering edge. Deprecation
policy for the two public ones must be decided before step 1: keep `slot()` as a
deprecated alias for one release, or take the major-version break.

---

## 2. Why `Memo`, not `Computed`

Both name the same slot. The decision is which property to foreground.

| | `Memo` | `Computed` |
|---|---|---|
| accuracy | one mismatch — classical memoization never invalidates — fixed by a "reactive" qualifier the namespace supplies | accurate unqualified |
| names | the mechanism | the role |
| lineage | Michie 1968; thunks, `lazy val`, `Lazy.t`, CAFs — **matches "lazily"** | spreadsheets, Swift properties, SQL computed columns |
| ecosystem | React, Solid, **Leptos**, `rmemo` | TC39, Vue, Angular, Preact, MobX |
| discriminates within the trio | ✅ — `Effect` is also "computed"; only `Memo` caches | ❌ — does not separate `Memo` from `Effect` |
| length | 4 | 8 |
| churn | constructor already correct | constructor renames in 8 bindings |

**Decisive precedent:** `leptos::reactive::computed::Memo` is this type, feature
for feature — lazy ("do not run at all until read... will not re-run until read
again"), cached, and equality-guarded ("will only notify its dependents if the
value of the computation changes", `PartialEq` by default). It is arena-allocated
and disposed by an `Owner`, with `ArcMemo` as the refcounted variant. That is
`lazily`'s design, in `lazily`'s language, named `Memo`.

There is **no ecosystem convergence at the type level** — TC39 says `Computed`,
Leptos says `Memo`. The convergence is on the *function* name, which does not
bind a type name.

**Prior objection, withdrawn:** "memoization never invalidates, and a reactive
memo is nullary." The nullary half is wrong — thunks, CAFs, and `lazy val` are
all zero-arity memoized, and standard. The invalidation half stands but is
answered by the qualifier, exactly as it is for `rmemo`.

---

## 3. Why `Slot` goes

Three collisions, three different meanings, in three ecosystems with bindings:

| ecosystem | "slot" means | binding affected |
|---|---|---|
| Python | `__slots__` — fixed attribute storage | `lazily-py` |
| Vue / Svelte / Web Components / Compose | `<slot>` — template insertion point | `lazily-js`, `lazily-react` |
| Qt | the **callback** a signal invokes | conceptual |

Joint reading of `Slot` + `Signal` is Qt's Signals & Slots — a callback system,
which is what this family spent a month proving it is not. Joint reading of
`Cell` + `Memo` is a spreadsheet, which is what a lazy reactive graph is.

**Process note worth institutionalizing:** nobody audited `Slot` against the
ecosystems `lazily` ships into before adopting it. That check belongs in the
spec's naming guidance. It has since killed `Latch` (C++20 `std::latch`, Java
`CountDownLatch`, plus the family's own "disposers latch") and `Input` (Python
builtin, HTML element, `InputStream`).

---

## 4. Why `Signal` goes

`Signal` has no position in the family's own taxonomy: not the genus
(`Reactive<T>` is), not a kind (it is a composition the arena has never heard
of).

**Verified against the proposal** (2026-07-20): TC39 defines

```ts
interface Signal<T> { get(): T; }
namespace Signal {
    class State<T> implements Signal<T> { ... }
    class Computed<T> implements Signal<T> { ... }
}
```

So `Signal<T>` is the **base interface every reactive implements** — structurally
identical to `lazily`'s `Reactive<T>`, with `State` adding `set` exactly as
`Source<T>` does. `lazily` uses the same word for **one specific composition**.
That is base-interface-versus-one-construction, not merely genus-versus-species.

**Not claimed:** the proposal contains no history indicating `Signal` was
demoted from a constructor to a namespace. Do not put that in the blog post.

---

## 5. The taxonomy, and where `Dependent` fits

### 5.1 The partition: incoming edges

| | writable | has dependencies | |
|---|---|---|---|
| `Cell` | ✅ | — | `Source` |
| `MergeCell` | ✅ | — | `Source` |
| `Memo` | — | ✅ | `Dependent` |
| `Effect` | — | ✅ | `Dependent` |

Every node is exactly one. Not a coincidence of the design — **a node's value
comes either from outside it or from upstream of it.** Write-fed or compute-fed,
never both, never neither.

`Reactive<T>` crosscuts rather than competing:

```
Reactive<T>   has a readable value   →  Cell, MergeCell, Memo   (not Effect)
Source        no incoming edges      →  MergeCell<M>   (Cell = MergeCell<KeepLatest>)
Dependent     has incoming edges     →  Memo, Effect
```

### 5.2 The three kinds are the three DAG positions

| | incoming | outgoing | dataflow role |
|---|---|---|---|
| `Cell` / `MergeCell` | — | ✅ | **source** |
| `Memo` | ✅ | ✅ | **interior / transform** |
| `Effect` | ✅ | — | **sink** |
| — | — | — | *isolated — nothing* |

The kernel is closed because the space is closed. `Effect` is the sink because
**nothing can ever depend on it** — no value to read, so no edge can point away.
Same fact explains why `Effect` cannot implement `Reactive<T>`.

Sink-ness is a **capability, not a current degree**: a `Memo` with zero
dependents is still not a sink; an `Effect` can never acquire one.

### 5.3 Source side collapses

Since `Cell ≡ MergeCell<KeepLatest>` (already in the spec), the kernel is **one
policy-parameterized source kind plus two dependent kinds**. `Cell` is an
instantiation, not a kind. The arena's three variants are an implementation
optimization the spec already permits.

---

## 6. `Dependent`: scope and justification

**What it is:** the shared seam for nodes with incoming edges — dependency
tracking, invalidation, re-run, edge teardown on disposal.

**Members** (all already implemented, independently, in multiple bindings):

- dependencies, dynamically re-discovered per run; degree introspection shipped
  2026-07-19
- staleness, and the invalidation that sets it
- a re-run entry point — `lazily-go` grew `Slot.refresh() bool` on a node
  interface 2026-07-20; `lazily-rs` has `refresh_slot`
- disposal detaching edges in both directions

**Why not `Computed` / `Computation` / `Computer` / `Latch`:**

| candidate | fails because |
|---|---|
| `Computed` | ecosystem-wide it means a derived **value**, excluding `Effect` (TC39 class, Leptos module doc: "Computed reactive values") |
| `Computation` | overloaded |
| `Computer` | "computes" does not discriminate — **`MergeCell` computes too** (`⊕(current, op)`), and so does `Cell` since `⊕ = replace`. Also collides with the most common noun in the field |
| `Latch` | C++20 `std::latch`, Java `CountDownLatch`, and the family's own "disposers latch". Also the wrong axis — a latch *stores*, which is what `Effect` doesn't do |

`Dependent` names the **edge relationship**, which is the property that actually
splits the space, and makes its own membership test obvious: "does a `MergeCell`
have dependencies?" is unarguable where "does a `MergeCell` compute?" is not. It
is already family vocabulary — `dependents_of`, `DependentCount`, "the dependent
cone."

**Visibility: internal until a public signature asks for it.** Callers hold a
`Memo` or an `Effect` and never need to say "either." Shipping it publicly would
put a fourth name in the documentation that isn't a node kind — the exact failure
`Signal` produced.

---

## 7. `Effect` → `MergeCell` (already specified, unfixtured)

Codified in `reactive-graph.md` 2026-07-20. Recorded here because the rename
touches the same prose.

**A `Source` never acquires a dependency edge.** "Feed this merge cell from that
reactive" is an `Effect` that reads and calls `merge`. The edge belongs to the
effect; the cell stays edge-free; the partition holds; no new node kind. Same
answer the family gave for eager values.

**Delivery is per settled cone, not per write.** `1 → 2 → 3` in a batch produces
**one** merge, of `3` — the intermediates were never materialized, because
eliding them is what a lazy graph is for.

| driver | merges performed | fold is over |
|---|---|---|
| explicit `merge()` calls | one per call, batched or not | **every op** — exact |
| `Topic` subscription | one per event | **every op** — exact |
| dependency edge, via an effect | one per settled cone | **settled values** — flush-granular |

The first two are exact because the *caller* decides how many ops exist; the
third is flush-granular because the *graph* decides. Same event-versus-state
split already recorded in `relaycell-backpressure-analysis.md`.

**`merge` folds synchronously inside a `batch`; only propagation defers.** N
calls → N folds, one invalidation. Pinned explicitly because "does batching lose
my merges?" is the accumulator case's first question. It does not.

---

## 8. Feedback loop analysis (already specified)

An effect reading `R` and merging into `M` upstream of `R` closes a loop through
the **scheduler**, not the graph. Not a dependency cycle, so acyclicity checks
never fire.

This is the family's **only** way to express feedback — the graph is acyclic by
construction, so a cycle cannot be an edge. Each iteration is one flush, making
it a bounded, observable discrete-time recurrence:

```
x_{n+1}  =  x_n ⊕ f(x_n)
```

**Termination is decided by the policy's algebra, which the policy already
declares:**

**Corrected 2026-07-20 after review — an earlier draft of this section, and of
the spec clause, overstated all three points below.** The spec is fixed; this is
the synced version.

| `⊕` | what the algebra gives you |
|---|---|
| semilattice join — idempotent, commutative, associative (CRDT policies) | **the ascent only.** `x_{n+1} ⊒ x_n` every step → non-decreasing chain. **Not termination.** |
| non-idempotent — `+`, `append`, counters | no *guaranteed* fixed point. Terminates iff `f` yields the identity (`0`, empty append) |

**ACC — the ascending chain condition.** Every strictly increasing chain

```
x₀ ⊏ x₁ ⊏ x₂ ⊏ …
```

is **finite**. Equivalently: there is no way to keep strictly growing forever.
A lattice of finite height satisfies it automatically. In plain terms for a merge
policy: *is there a bound on how many times the accumulated value can strictly
increase before it must repeat?*

| value domain | ACC | why |
|---|---|---|
| boolean flag `false ⊑ true` | ✅ | height 1 |
| max over a fixed finite range | ✅ | bounded above |
| set over a **finite** universe | ✅ | bounded by universe size |
| G-Set / OR-Set over strings, UUIDs | ❌ | always room for a fresh element |
| LWW with unbounded timestamps | ❌ | always a later timestamp |
| append-only log | ❌ | always longer |

**The join does not supply ACC.** Idempotence, commutativity and associativity
constrain *how* values combine; they say nothing about how tall the lattice is. A
G-Set over an unbounded domain is a perfectly good join semilattice whose chain
climbs forever if `f` keeps producing fresh elements. **Most CRDT policies are
not finite-height** — which is by design, since a CRDT that could only grow
finitely would be a poor replicated set.

So a policy declaration is not evidence that a loop over it terminates. ACC is a
separate property of the **value domain**, and it is the caller's to establish.

**Not the classical dataflow result.** Kleene/MFP requires a monotone transfer
function and delivers the *least* fixed point. This reaches *a* fixed point, with
no leastness guarantee, precisely because `f` need not be monotone.

**The `==` store-guard detects a fixpoint of `==`, not of the lattice order.** A
policy carrying tombstones, causal context or vector clocks can compare unequal
at a genuine lattice fixpoint, and spin.

### 8.1 Deliberately unbounded loops, and where an interrupt can land

"Run until cancelled" is a legitimate pattern — simulation steps, iterative
solvers, pollers. It is permitted; the caller just owes a stated reason the loop
ends. **Where that check can live is not obvious, and differs by context.**

A scheduler-closed loop iterates *inside the flush*. In a synchronous context the
flush does not return to the caller between iterations — `set_cell` → flush →
effect → merge → invalidate → flush → … is one un-returned call. So the calling
thread is not running and cannot observe an interrupt. It also starves whatever
would deliver one. That is livelock, not iteration.

**The yield point that does exist is the effect body**, which is caller code
running once per iteration. Not writing is what ends the cascade:

```
effect(|ctx| {
    if cancelled.load() { return }   // no merge → no write → no invalidation → drain ends
    merge(acc, f(ctx.get(upstream)))
})
```

| mechanism | sync context | async context |
|---|---|---|
| flag / deadline / counter checked in the effect body | ✅ **the** yield point | ✅ |
| external cancellation between iterations | ❌ no yield point; caller isn't running | ✅ awaits provide one |
| binding-level flush-re-entry bound | ⚠️ backstop only — reports *exhaustion*, which is not convergence | ⚠️ same |

Accordingly, per the corrected spec:

- A caller **MUST** be able to state why the loop terminates — ACC on the value
  domain for a join policy, or an explicit condition otherwise. **The policy's
  declared algebra is not sufficient evidence.**
- Bindings **SHOULD** bound effect-flush re-entry and report exhaustion.
  **Unmeasured: no binding is known to do this, and nobody has checked.**

**Fixture debt:** none of §8 is fixtured, including the re-entry bound. Add to
the §9.4 set — at minimum, a divergent loop must produce a reported exhaustion
rather than a hang, and that is testable with a bounded iteration count.

---

## 9. Conformance fixtures

The corpus is a **shared contract between the spec and eight runners**, so it
does not rename like source does. Measured 2026-07-20.

### 9.1 What is already neutral

**The derivation op is already `computed`, not `slot`.** The 18 ops in
`_REACTIVE_GRAPH_OPS` are `cell`, `computed`, `effect`, `read`, `set_cell`,
`dispose`, `batch`, `begin_scope`, `end_scope`, `disarm`, `churn`, `fanout`,
`dispose_fanout`, `dispose_stale_handle`, `subscribe`, `unsubscribe` (retired
observer ops), `signal`, `dispose_signal`.

So the corpus never adopted `slot` as an op name, and the rename touches **no op
that describes the derivation**. That is a large piece of luck.

### 9.2 What must change

| item | count | notes |
|---|---|---|
| `signal` op → `eager_memo` | 1 op, 3 fixtures, **8 runners** | flag day, see 9.3 |
| `dispose_signal` op → `dispose_eager_memo` | 1 op, 1 fixture, **8 runners** | flag day |
| literal "slot" in fixture prose / node ids | **47 occurrences, 13 files** | 8 in `reactive-graph/`, 2 in `materialization/`, 1 each in `collections/` and `reliable-sync/`, plus top-level `delta_*` — descriptive text and some node ids, no semantics |
| `_REACTIVE_GRAPH_OPS` allowlist | 1 file | `tests/test_schema_conformance.py` — needs the eager rename **and** a new `merge` op, which is not currently in the allowlist |
| new `MergeCell`-fed-from-a-reactive fixtures | **4 new** | §7 properties, currently unfixtured |

### 9.3 Op renames are a flag day — plan for it

Unlike a type rename, which is internal to one binding, **a fixture op rename
breaks every runner at once.** The corpus is shared and canonical: the moment a
fixture says `eager_memo`, all eight bindings fail it until all eight are
updated. There is no per-binding migration window.

Three options:

1. **Don't rename the ops.** `signal` / `dispose_signal` stay as op names
   describing the eager construction, while the *type* becomes `Memo`. Zero
   coordination cost. The cost is a vocabulary seam — the corpus keeps a word the
   library retired.
2. **Transitional dual-accept.** Runners accept both `signal` and `eager_memo`;
   fixtures switch; the old name is dropped a release later. No flag day, at the
   cost of one release carrying both.
3. **Lockstep.** All eight in one pass. Cheapest to reason about, riskiest to
   execute, and it means the corpus is red in every repo until the last binding
   lands.

**Recommended: (2).** It is the only option that is neither a seam nor a flag
day, and dual-accept is ~3 lines per runner. **(1) is a defensible fallback** —
the op vocabulary is a test-harness contract, not public API, and leaving
`signal` there costs nothing a reader will trip over.

### 9.4 The four new fixtures

For §7, currently normative prose with no fixture — the exact state the signal
semantics sat in for months while a binding shipped a different construction
under a green checkmark.

1. **One merge per settled cone, not per write.** `1 → 2 → 3` in a batch → one
   merge of `3`. Must use a **non-idempotent policy** (`+`), because values
   converge either way under `KeepLatest` and only the count separates a
   conforming binding from a per-write one. Assert on merge count, not value.
2. **`merge` folds synchronously inside a batch.** N calls → N folds, one
   invalidation. Catches a binding that defers the fold itself.
3. **The cell acquires no dependency edge.** `dependents_of` /
   `dependencies_of` on the merge cell stays 0 after the construction — pins the
   `Source`/`Dependent` partition structurally rather than by prose.
4. **Exact-fold paths stay exact.** Explicit calls and `Topic`-driven merges fold
   every op regardless of batching, in contrast to (1).

Runner surface needed is small: a `merge` op plus a policy parameter on the cell
op. The counting machinery already exists in all eight runners from
`computes_of`.

### 9.5 Verification discipline, carried over

- **Mutation-verify each new fixture.** Break the implementation deliberately and
  confirm the fixture goes red. Established 2026-07-19 after two bindings found
  the disposal corpus pinned one of three semantics it was named after, 9-of-9
  green.
- **Every replay prints fixture counts.** A green suite in an unfamiliar
  environment is an empty suite until it says how much it ran.
- **No fixture, clause, or assertion is weakened to make a binding pass.** A
  reported divergence is a successful outcome.

---

## 10. Execution

**Two independent tracks.** An earlier draft bundled them and produced an
unachievable gate — step 1 required `make check` green on fixtures using a
`merge` op no runner implements. They are now separate and can proceed in
parallel.

### Track A — the rename

Ordering is driven by §9.3's **dual-accept** recommendation, which requires
runners to accept the new op *before* any fixture emits it. An earlier draft
sequenced fixtures first, which is the lockstep flag day §9.3 says to avoid.

1. **Decide the two open policy questions** (§1, §13): deprecation for
   `lazily-js`'s public `slot()` / `disposeSlot()`, and `SlotValue` — leave the
   seam or break the wire.
2. **Spec prose only** — no fixture op changes yet. `docs/reactive-graph.md`,
   root `cell-model.md` and `protocol.md` (**mind the `docs/` symlinks — do not
   double-edit**), `README.md`, `coverage.json`, `book/`, `examples/`. `make
   check` green.
3. **Formal.** **19 files**, not 2. `AsyncSlotState.lean` (74 hits) is a **file
   rename**. `Signaling.lean` and `SignalingRoster.lean` are **WebRTC signaling —
   semantically unrelated, do not touch.** `lake build` clean, no `sorry`.
4. **All nine runners dual-accept** `signal` **and** `eager_memo`. ~3 lines each.
   Corpus stays green throughout; no fixture has changed yet.
5. **Flip the fixtures** to `eager_memo` / `dispose_eager_memo`, plus the 47
   prose occurrences across 13 files. Corpus green everywhere on the first run,
   because step 4 already landed.
6. **Reference binding** `lazily-rs` — type, handles, `SlotMap`, `eager_memo`,
   and the `Dependent` trait as an internal seam. ~8,400 sites: use a mechanical
   substitution, having first confirmed no slab/arena sense of "slot" exists
   (verified zero hits today). Include `benches/` — they feed `BENCHMARKS.md`
   and `#lzbenchver` gates on drift.
7. **`lazily-js`**, honouring the deprecation decision from step 1.
8. **`lazily-react`** — depends on step 7 for `MemoHandle` types.
9. **Remaining six in parallel** — separate repos, **one agent per repo, never
   two**. Each replays the full corpus and prints counts as proof it ran.
10. **Drop the deprecated op alias** a release later.
11. **Blog** (§11), then superproject pointers, then deploy.

### Track B — the MergeCell fixtures

Independent of the rename; do not bundle. Needs new runner surface.

1. Add the `merge` op and a cell policy parameter to `_REACTIVE_GRAPH_OPS` —
   **`merge` is not currently in the allowlist.**
2. Implement the op in all nine runners.
3. Land the four §9.4 fixtures, mutation-verified.
4. Add a re-entry-bound fixture per §8.1: a divergent loop must report exhaustion
   rather than hang, testable with a bounded iteration count. **Currently
   unmeasured in every binding.**

### Gates, both tracks

- **The corpus must print its fixture counts.** A green suite in an unfamiliar
  environment is an empty suite until it says how much it ran — an isolated
  worktree silently skipped the entire corpus at exit 0 on 2026-07-20.
- **RTK filters `go test` output.** Use `rtk proxy go test -v` for counts, and
  capture `$?` directly rather than through a pipe.
- **Nothing is weakened to make a binding pass.** A reported divergence is a
  successful outcome.

---

## 11. Blog impact

The post *Signal Is Not a Reactive Primitive* **survives** — its "Signal" is the
ecosystem's word, and retiring the name vindicates the title rather than
nullifying it. Keep the title. Three edits owed:

1. "I am not arguing that frameworks should stop exposing `signal()`. `lazily`
   exposes it too, and will keep exposing it" — becomes false. Replace with: the
   composition still deserves a constructor, at the right taxonomic level, so it
   is `eager_memo` and the word `Signal` returns to the ecosystem.
2. The opening Rust snippet and `SignalHandle` discussion become historical —
   keep with a note; the argument is about what the construction *is*.
3. Add a closing section: the family stopped shipping the name. That is the
   post's own rule — treat an ambiguous, conflicting, or inaccurate name as a bug
   report about the architecture — applied reflexively.

---

## 12. Non-goals

Explicitly out of scope. Each is a separate decision, deliberately not bundled —
bundling is how a rename becomes a spiral.

- **`ctx.pull(computed)` returning a disposable.** The most thesis-consistent
  shape (no eager type; eagerness is something you *do*) and it would fix the
  `dispose_signal` misnomer. **An API design question — judge it without a
  migration riding on it.**
- **`Cell` → anything.** Survives the spreadsheet-genus objection: the name
  borrows *containment at a position*, and `Cell`/`Memo` is an unmarked/marked
  pair like SQL `column`/`computed column` and Swift property/`computed
  property`.
- **`Source` → `Input`.** `Source` is correct in two senses at once — writable,
  and graph-theoretically a node with no incoming edges — and those coincide by
  construction. `Input` collides with the Python builtin, the HTML element, and
  `InputStream`.
- **Making `Dependent` public.**
- **`lazily-go`'s async plane** — still value-only suppression; see
  [`lazily-signal-followups.md`](lazily-signal-followups.md) §1.

---

## 13. Risk

**Not low. An earlier draft said "low, and lower than it looked" — that was
wrong on three counts**, all found in review.

**Volume.** Word-boundary `slot` occurrences per repo:

| repo | sites | | repo | sites |
|---|---|---|---|---|
| `lazily-rs` | **8408** | | `lazily-kt` | 370 |
| `lazily-py` | 1116 | | `lazily-js` | 276 |
| `lazily-zig` | 851 | | `lazily-cpp` | 275 |
| `lazily-dart` | 530 | | `lazily-react` | 39 |
| `lazily-go` | 378 | | | |

~12,000 sites. In `lazily-rs` the density is `src/context.rs` (174),
`src/thread_safe.rs` (168), `tests/spec_compliance.rs` (161),
`src/async_context.rs` (143), plus `benches/context.rs` (30) and
`benches/async_context.rs` (24) — **benchmarks feed `BENCHMARKS.md`, and
`#lzbenchver` already gates on version drift.** A mechanical strategy is
required, not a per-file pass.

Mitigating: a search for slab/arena-index senses of "slot" in `lazily-rs`
(`free_slot`, `slot_index`, `slots[`, `slab`) found **zero hits**, so the
substitution is cleaner than the count suggests. Verify the same per binding
before starting.

**Public API.** Two repos ship `slot` publicly (§1). This is a breaking npm
change, not a rename.

**Wire vocabulary.** `slot_id` is genuinely retired and actively rejected — that
claim held. But the delta format still carries a live variant named `SlotValue`
(`schemas/delta.json:34,109`; 10 references in `protocol.md`), and
`protocol.md:206` already mixes vocabulary: "**Memo** equality suppression: a
dirty `memo()` … emits no `SlotValue`". **Decide explicitly:**

- (a) leave `SlotValue` — permanent wire-level seam, contradicts §11.3's "the
  family stopped shipping the name"; or
- (b) rename it — a breaking wire change across all bindings **plus `agent-doc`'s
  IPC**, which consumes this format.

Not deciding is not an option; the earlier "no wire change" headline hid it.

**Docs.** `lazily-spec` alone: `README.md` (18.6K), `coverage.json`, `book/`,
`examples/`, and 30+ files under `docs/` including `reactive-graph.md` (68K) and
`relaycell-backpressure-analysis.md` (36K). Note `cell-model.md` and
`protocol.md` live at repo root with symlinks in `docs/` — **an agent doing
path-based edits must not double-edit through both paths.**

**The real risk remains scope creep.** Every alternative in §12 is more
interesting than the rename, and none belongs in it. §10 previously violated this
itself by scheduling the new `MergeCell` fixtures inside the rename step; that is
now split into an independent track.
