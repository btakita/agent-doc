# lazily — follow-ups from the Signal conformance pass (2026-07-20)

Opened after `#lzsignaleager`: four normative Signal clauses added to
`lazily-spec/docs/reactive-graph.md`, three fixtures added, 12 Lean theorems in
`lazily-formal/LazilyFormal/Signal.lean`, and the corpus replayed across all
eight bindings.

Clause 3 (a batch of N writes produces exactly ONE compute, at batch exit) caught
**four bindings across two unrelated mechanisms**. Every one produced correct
values while doing 2–3× the computes, which is why no other fixture saw them.

---

## 1. `#lzgomarkfrontier` — migrate lazily-go to a non-consuming cascade

**Status:** assessed, deliberately not started. This is the big one.

lazily-go cannot satisfy Signal clause 3 and equal-recompute suppression at the
same time. `Memo` implements the `==` guard by recomputing during invalidation,
and underneath that, the cascade **consumes reverse edges as it walks**:

- `reactiveBase.invalidate` (core.go:116) — `clear(b.dependents)`
- `AsyncContext.propagate` (async_context.go:618) — `delete(c.dependents, cur)`

Pull-time suppression requires marking a dependent clean without recomputing. A
dependent that does not recompute never re-runs `track()`, so it never
re-registers, so its source can no longer reach it and the next genuine change is
lost at depth two. That is `hybrid_serves_stale_value_at_depth_two` in
`lazily-formal`.

`Effect.relinkDependencies` exists solely to repair edges the consuming cascade
ate. Its own comment notes lazily-rs needs no equivalent because its mark-frontier
walk does not consume.

**Blast radius**

1. Two independent engines — sync (per-node edge sets, `cached bool`) and async
   (context-owned `c.dependents`, 4-state FSM with revisions, in-flight tokens,
   waiters). Both, or the planes diverge further.
2. Both terminate *by* edge consumption; non-consuming marking needs a
   visited/dirty short-circuit added to each walk.
3. `cached bool` conflates "no value" with "dirty" — pull-time checking needs
   three states. ~60 occurrences across 9 non-test files, including three
   ReactiveMap implementations, `sem_tree.go`, and `Context.cachedCount` behind
   the public `Size()`.
4. `DependentCount` is a public observable calibrated to the consuming cascade —
   30 `dependents_of` assertions across 8 reactive-graph fixtures are written
   against post-cascade degree readings.

**Resolve in the same pass:** go's two planes already disagree about what the
memo guard means. `Context` suppresses downstream entirely (0 recomputes on an
equal memo recompute); `AsyncContext` does value-only suppression (1), because
`propagate` walks the full transitive cone before `onComplete` applies `eq`. So
sync *relocates* suppression while async *adds* downstream suppression it has
never had.

**Rejected shortcut — do not take it.** Keep the consuming cascade, have
`Memo.invalidate` mark dependents and defer the guard to effect-flush, then
un-mark if equal via `relinkDependencies`. Passes all four clauses without
touching propagation, but it is a third variant of one-level-marking-plus-repair
— the exact hybrid family the Lean theorem is about — and puts the guard at flush
time rather than pull time. It would satisfy the fixtures while diverging from
the proven model.

**Starting point already in the tree**

- `TestPullTimeMemoGuard_Target` — the failing target test, restored and skipped,
  pointing at the ledger entry.
- `TestPullTimeMemoGuard_SuppressionAndNoWriteLoss` — passes TODAY on eager
  marking and must stay green at *every* step. A half-migration cannot keep it
  green; that is the tripwire.
- A bidirectional known-divergence ledger recording current behavior, which goes
  red when the behavior changes.

**Done when:** non-consuming cascade on both planes, pull-time memo guard,
`Signal` switched to Memo backing (spec-literal), equality ledger retired, all
three signal fixtures plus `transitive_invalidation_reaches_depth` green on every
context, 30 `dependents_of` assertions re-derived.

---

## 2. `Slot` → `Computed`, and retire `Signal`

**Status:** converged 2026-07-20, not started. One pass, both renames together —
they are the same argument applied twice.

**Decided:**

| | |
|---|---|
| genus | **`Reactive<T>`** — already the spec's umbrella (reactive-graph.md:47), with `Source<T>: Reactive<T>` for the writable ones. Preferred over `Signal`. |
| source | **`Cell`** — keep. Borrows *containment at a position*, which holds exactly (arena id holding a value), and is the unmarked member of an unmarked/marked pair. `State` would collide with this family's own statechart module. |
| derivation | **`Slot` → `Computed`** |
| sink | **`Effect`** — keep. |
| eager form | **`Signal` → `Computed.eager`** — retire the word. |

**Why `Signal` goes.** It has no slot in the family's own taxonomy: not the genus
(`Reactive<T>` is), not a kind (it is a composition the arena has never heard
of). In TC39 `Signal` is the *category* — `Signal.State` reads "reactive state",
`Signal.Computed` reads "reactive computed" — so the family is using the word one
taxonomic level below the standard it cites as corroboration, naming a species
with the genus's word. The `Signal.` qualifier exists there only because TC39
ships into a global namespace; a library namespace supplies it for free, which is
why Vue and Angular say `computed` with nothing in front.

Retiring it also closes the last of the three meanings catalogued in the
*Signal Is Not a Reactive Primitive* post — the family stops participating in the
ambiguity rather than documenting it from inside.

**Why `Computed`, despite the length.** It names the role; `memo` names a
technique whose classical meaning (Michie 1968) *never invalidates*, which is the
central feature of this node. Precedent is exact at the interface level: Swift
stored-vs-computed properties, SQL computed columns (virtual by default,
`PERSISTED` on request — lazy vs eager, in a standards-track feature). Rejected:
`Derived` (collides with the spec's own "derived construct" prose), `Formula`
(names the expression, not the memoized value), `Memo`, and abbreviations.

The length is a real cost, paid once per derivation at the call site and
recovered on every read.

**Why `Slot` goes.** Three collisions, three different meanings, in three
ecosystems this family ships bindings into: Python `__slots__` (attribute
storage), Vue/Svelte/Web Components/Compose `<slot>` (template insertion point),
Qt (the callback a signal invokes — the thing the family banned). Joint reading
of `Slot` + `Signal` is Qt's Signals & Slots, a callback system. Joint reading of
`Cell` + `Computed` is a spreadsheet, which is what a lazy reactive graph is.

**On the `Cell` objection, since it will be raised again.** In a spreadsheet
"cell" is the genus — A1 is a cell whether it holds `42` or `=B1+C1` — so
narrowing it to the entered case looks like the same category error that retires
`Signal`. It is not, for two reasons. The name borrows *containment at a
position*, which holds exactly for both (an arena id holding a value), rather
than borrowing entered-ness. And `Cell`/`Computed` is an **unmarked/marked
pair**: the bare container noun for the source, the participle marking provenance
for the derivation. That construction is standard in the immediate neighbourhood
— SQL `column` / `computed column`, Swift property / `computed property`,
spreadsheet "cell" / "formula cell" — and in all three the unmarked form means
stored. Supporting precedent: outside spreadsheets, `Cell` in programming means a
*mutable container* specifically (Rust `Cell`/`RefCell`, Lisp cons cells, OCaml
ref cells), which is narrow in the right direction. And the interface layer
states the real relationship anyway: `Reactive<T>` is the genus, `Source<T>` the
writable branch, `Cell ≡ MergeCell<KeepLatest>` one concrete instance.

`Slot` collides in three ecosystems lazily ships bindings into, with three
different meanings: Python `__slots__` (attribute storage), Vue/Svelte/Web
Components/Compose `<slot>` (template insertion point), Qt (the callback a signal
invokes — the thing the family banned). `Computed` collides with nothing and is
what TC39, Vue, Angular, Preact, and MobX already mean by it.

`Computed` names the role; `memo` names the technique. `Cell`/`Computed`/`Effect`
is a taxonomy of roles; `Cell`/`Memo`/`Effect` mixes a role, a strategy, and a
role.

**The eager form: a concept decision, spelled per binding.**

Settled: **eagerness is a marked modifier on a computed, not a kind.** Lazy stays
unmarked. `ctx.computed.lazy(f)` was considered and rejected — putting lazy and
eager side by side as peer options asserts a symmetry the *Signal Is Not a
Reactive Primitive* argument explicitly denies (`eager = lazy + puller`,
`lazy ≠ eager - anything`). Unmarked-lazy encodes the asymmetry for free and
matches the `Cell`/`Computed` unmarked/marked pattern.

NOT settled, and deliberately left per-binding: the spelling. A hard constraint
drives this — **Go methods cannot have type parameters**, so every dotted form
(`ctx.computed.eager[T](f)`, `ctx.eager.computed[T](f)`) is impossible there for
generic constructors. That is why lazily-go already uses free functions
(`NewMemo[T](ctx, compute)`) rather than context methods, and has always
diverged from the `ctx.method()` convention out of necessity.

Also rejected: `ctx.computed.eager(f)` as a family syntax. It needs `ctx.computed`
to be simultaneously callable and method-carrying, which is nightly-only in Rust
(`fn_traits`), impossible in Go for generics, and unavailable in Zig. Safe in
five bindings and unavailable in three is the wrong family default — the same
principle that resolved the disposal contract.

**DECIDED: `ctx.computed(f)` + `ctx.eager_computed(f)`** — two flat constructors,
cased per language (`eagerComputed` in JS/Kotlin/Dart/Zig, `EagerComputed[T](ctx,
f)` as a free function in Go, which is what Go already does for every generic
constructor).

The decisive argument is scope discipline, not elegance. Today's API is
`ctx.memo(f)` + `ctx.signal(f)` — two flat constructors. This keeps that shape
and changes only the words, so the rename stays a rename. Every alternative below
converts a naming pass into an API redesign on top of a change already touching
8 bindings, the wire protocol, 114 fixtures, the Lean model, and two published
posts. That is how a rename becomes a spiral.

It also preserves unmarked/marked, needs no new machinery (no namespace object,
no ZST wrapper, no callable struct, no caller-managed disposable), and works in
all eight with only the casing/free-function transformation already in use.

Alternatives considered and deferred — evaluate separately, never bundled:

- `ctx.eager.computed(f)` / `ctx.eager().computed(f)` — modifier-then-noun;
  works wherever methods can carry type parameters.
- `ctx.pull(computed)` returning a disposable — the most thesis-consistent: no
  eager type at all, eagerness is something you *do*, and it would fix the
  `dispose_signal` misnomer as a side effect. Works in all eight including Go.
  **Worth revisiting on its own once the rename has landed** — it is an API
  design question, not a naming one, and it deserves to be judged without a
  migration riding on it.

Precedent for specifying the concept and leaving the spelling: reactive-graph.md
already does this for `batch`, noting that closure-based bindings take the writes
as the closure body while begin/end bindings call them around it.

**Still to settle before the first edit:**

- Constructor: `ctx.computed()`, with `ctx.memo()` kept as a deprecated alias for
  one release? (The type/constructor asymmetry is the reason to change it at all
  — `ctx.cell()` → `Cell` and `ctx.effect()` → `Effect` already match.)
- ~~Wire protocol `slot_id`~~ — **RESOLVED, no work needed.** Checked
  2026-07-20: the wire protocol already addresses nodes with a kind-agnostic
  `node` field typed as `NodeId` (`schemas/delta.json`), which covers Cell, Slot
  and Effect alike. `slot_id` is not merely deprecated there but a **stale form
  the schema actively rejects**, guarded by
  `test_stale_snapshot_form_with_slot_id_is_rejected`. Someone already made this
  generalization and went past `reactive_id` to plain `node`.

  **So the rename has no wire component at all** — no serialized field change, no
  compatibility break, no legacy-identifier decision. It is a pure source, docs,
  fixtures, and prose change, which materially shrinks the risk.

  The only surviving `slot_id` is in `schemas/agent-doc-state.json`, describing
  an application-level hash in the agent-doc state projection
  (`slot_id = fnv1a(document_hash, type_tag, entity_key)`). That is agent-doc's
  vocabulary, not lazily's node addressing, and is a separate decision.

**Scope:** 8 bindings, spec prose, 114 fixtures, the Lean model, both blog posts,
the wire protocol.

**Blog: the post titled *Signal Is Not a Reactive Primitive* survives the
rename** — its "Signal" is the ecosystem's word (TC39, SolidJS, Preact, Angular,
`rmemo`), and lazily is the evidence rather than the subject. Retiring the name
*vindicates* the title: the post argues Signal is a composition that does not
belong in the kernel, and the library then removed it from the primitive
namespace. Keep the title.

Three edits owed at rename time:

1. This passage becomes false and must change — "I am not arguing that
   frameworks should stop exposing `signal()`. `lazily` exposes it too, and will
   keep exposing it." Replace with: the composition still deserves a
   constructor, because the puller-must-be-an-effect invariant is easy to get
   wrong, but it deserves a name at the right taxonomic level, so it is
   `Computed.eager` and the word `Signal` goes back to the ecosystem that uses it
   for the category.
2. The opening five-line Rust snippet and the `SignalHandle` discussion become
   historical. They can stay — the argument is about what the construction *is*,
   not how it is spelled — with a note.
3. Add a closing section: the family stopped shipping the name. That is the
   post's own rule ("treat an ambiguous, conflicting, or inaccurate name as a bug
   report about the architecture") applied reflexively, and it is a better ending
   than the post currently has.

**Process note worth institutionalizing:** nobody audited `Slot` against the
ecosystems lazily ships into before adopting it. That check belongs in the spec's
naming guidance next to the `unmeasured` discipline.

---

## 3. Smaller findings, each independent

- **lazily-dart exposes `Signal` as a public node kind** (`class Signal<T> extends
  _ReactiveNode`). The spec's rule is now a `SHOULD NOT` rather than a `MUST NOT`
  precisely because the base class is private and the property is therefore not
  caller-observable — so this is a design question about keeping the kernel
  closed, not a conformance failure. Decide whether to restructure; it is a
  public API break.
- **lazily-zig `AsyncContext` has no lazy mode.** `setCell` walks `reverse_edges`
  and enqueues every dependent unconditionally with no demand check, so every
  derived slot is eager and disposing a puller removes nothing observable. Clause
  4 is ledgered as a divergence there. Fixing it means demand-driven slots — a
  rewrite of the async scheduler, with `reliable_sync` and `async_reactive_map`
  riding the current drain discipline.
- **lazily-cpp `AsyncContext` is a stub** and exposes no signal API at all: its
  async slot node carries no dependents/dependencies, `get_async` recomputes
  unconditionally, and `get()` serves a cache nothing invalidates. Already
  recorded in the capability table. Either implement a dependency graph or
  declare `async: none`.
- **lazily-kt `AsyncContext` ships only `disposeSignalNode`** (tears down both
  halves) where the other contexts expose a puller-only `disposeSignal`. Adding
  it is a one-liner but widens public API — decide deliberately.
- **The async surface is the family's weakest, systematically.** Four of eight
  bindings have an async context that is a stub, has no lazy mode, or ships no
  signal constructor. Worth a dedicated audit rather than finding it one clause
  at a time.

---

## 4. Fixture the MergeCell-fed-from-a-reactive clause

Added to `reactive-graph.md` 2026-07-20 as normative prose, **explicitly marked
not yet fixtured**. That is the same state the signal semantics sat in for
months — detailed prose, zero MUST/SHOULD enforcement, no fixture — while a
binding shipped a structurally different construction under a green checkmark.
Do not let this one sit as long.

Four properties worth pinning, in rough priority:

1. **One merge per settled cone, not per write.** `1 → 2 → 3` inside a batch
   produces one merge of `3`. Discriminating against a binding that delivers
   per-write. Needs `computes_of`-style counting on the merge, not values —
   values converge either way for `KeepLatest`, so use a non-idempotent policy
   (`+`) where 3 versus 6 separates them.
2. **`merge` folds synchronously inside a batch; only propagation defers.** N
   calls inside a batch → N folds, ONE invalidation. Catches a binding that
   defers the fold itself.
3. **The cell acquires no dependency edge.** `dependents_of` / `dependencies_of`
   on the merge cell stays 0 after the construction — pins the Source/Dependent
   partition structurally.
4. **Exact-fold paths stay exact.** Explicit calls and `Topic`-driven merges fold
   every op regardless of batching, in contrast to (1).

The runner surface needed is small — a `merge` op and a policy parameter on the
cell op. Nothing like the `computes_of` lift, since the counting machinery now
exists in all eight runners.

Also unenforced and worth a thought: bindings **SHOULD** bound effect-flush
re-entry, because an effect reading `R` and merging into `M` upstream of `R` is a
feedback loop closed through the scheduler rather than the graph — invisible to
cycle detection, manifesting as a non-terminating flush. No binding is known to
bound this today; nobody has checked.

## 5. Mutation-verify the rest of the corpus

The disposal area was found to pin one of three semantics it is named after,
9-of-9 green, discovered only by mutation. The signal fixtures were mutation-
verified in js, kt, cpp, dart, py, and rs as they landed. **Nothing says the
other ~19 areas are better.** This is the largest open conformance item and it
replaces the replay gap, which closed 2026-07-19.
