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

## 2. `Slot` → `Computed` rename

**Status:** decided in principle, not started. Its own pass.

`Slot` collides in three ecosystems lazily ships bindings into, with three
different meanings: Python `__slots__` (attribute storage), Vue/Svelte/Web
Components/Compose `<slot>` (template insertion point), Qt (the callback a signal
invokes — the thing the family banned). `Computed` collides with nothing and is
what TC39, Vue, Angular, Preact, and MobX already mean by it.

`Computed` names the role; `memo` names the technique. `Cell`/`Computed`/`Effect`
is a taxonomy of roles; `Cell`/`Memo`/`Effect` mixes a role, a strategy, and a
role.

**Settle before the first edit:**

- Constructor: `ctx.computed()`, with `ctx.memo()` kept as a deprecated alias for
  one release?
- Wire protocol `slot_id` — rename the serialized field, or keep it as a legacy
  identifier with a note? This is the expensive part.
- Does `Signal` survive as a name at all, or become `Computed.eager`? Separate,
  bigger decision — the word carries three meanings across ecosystems.

**Scope:** 8 bindings, spec prose, 114 fixtures, the Lean model, both blog posts,
the wire protocol.

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

## 4. Mutation-verify the rest of the corpus

The disposal area was found to pin one of three semantics it is named after,
9-of-9 green, discovered only by mutation. The signal fixtures were mutation-
verified in js, kt, cpp, dart, py, and rs as they landed. **Nothing says the
other ~19 areas are better.** This is the largest open conformance item and it
replaces the replay gap, which closed 2026-07-19.
