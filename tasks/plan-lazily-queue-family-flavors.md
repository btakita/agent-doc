# Plan — `QueueCell` / `TopicCell` / `WorkQueueCell` across all three execution flavors

Status: **planned, not started.** Sibling of
[`plan-lazily-reactivemap-unification.md`](plan-lazily-reactivemap-unification.md)
— same defect shape (a Core surface that only the single-threaded flavor ships),
same remedy shape (spec Core/Extended split → per-flavor fixture replay →
reference binding → fan-out), but a **worse** starting position: two bindings
score green on the queue-family coverage rows while shipping no reactivity at
all, and one binding has no queue family whatsoever.

Ask that produced this plan: *"Add `AsyncContext` + `ThreadSafeContext` support
to `QueueCell` + `TopicCell` + `WorkQueueCell`. Start with spec + formal +
conformance. Then subagents for implementations. Ensure consistency across sync
+ thread + async APIs."*

## What exists today

Surveyed read-only across all nine bindings. **No binding ships a thread-safe or
async flavor of any queue-family primitive.** Every one is hard-bound to the
single-threaded context:

| binding | `QueueCell` | `TopicCell` | `WorkQueueCell` | context | reader-kinds are… |
|---|---|---|---|---|---|
| rs | ✅ | ✅ | ✅ | `&Context` | memoized `Computed` + `Source<bool> closed`, invalidated by `clear_slots` |
| py | ✅ | ✅ | ✅ | `ctx: dict` (type-erased) | `Slot` + `Cell`, invalidated by `.reset(ctx)` |
| go | ✅ | ✅ | ✅ | `*Context` | `NewSlot` + `NewSource` |
| dart | ✅ | ✅ | ✅ | `Context` | `Slot` + `Source` |
| kt | ✅ | ✅ | ✅ | `Context` | `ctx.computed` + `ctx.source` |
| cpp | ✅ | ✅ | ✅ | `Context&` | **`Source<uint64_t>` version cells**, reads `ctx.get` them |
| zig | ✅ | ✅ | ✅ | holds `*Context`, **never mints a node** | **plain `u64` counters callers poll** |
| js | ✅ | ✅ | ✅ | **none — zero `ctx` references** | **a snapshot diff**, no graph |
| cs | — | — | — | — | family absent |

`coverage.json` scores all three queue rows `✅` for eight bindings and `—` for
cs. Those eight marks conflate three materially different designs.

### Finding 1 — the coverage row is green on two bindings that are not reactive

- **js**: `src/queue.js` `QueueCell` takes no context (`constructor(initial = {},
  storage)`), contains **zero** occurrences of `ctx`, and reports invalidation by
  diffing a private snapshot against the previous one. Nothing subscribes;
  nothing invalidates a graph. This is the same class of finding as the
  ReactiveMap survey's "js never replays the collections fixtures against a
  reactive map at all" — the fixture is satisfied by a non-reactive model.
- **zig**: `src/lazily/queue.zig` `QueueCell` stores a `*Context` but mints no
  node. Reader-kinds are five `u64` version counters (`head_version`,
  `len_version`, …) that callers must poll. Identical to the defect the
  ReactiveMap plan already catalogues for zig's sync map: *the richest surface is
  the least reactive.*

**Consequence for sequencing:** adding flavors to js and zig first would land
green against nothing. Their reactivity gap is a prerequisite, not a follow-on.

### Finding 2 — three incompatible reader-kind designs are all "conformant"

1. **Memoized-derive + explicit invalidate** (rs, kt, py, go, dart): reader-kinds
   are memoized `Computed`/`Slot`s with no graph dependencies; an op clears
   exactly the ones whose value provably changed. This is what
   `cell-model.md` § *Demand-driven derivation* describes.
2. **Version-cell subscribe** (cpp): reader-kinds are `Source<uint64_t>` bumped
   per op; a read does `ctx.get(version_cell)` for the edge and then derives from
   storage. Observably equivalent, structurally different.
3. **Poll a counter** (zig) / **diff a snapshot** (js): not reactive.

The user's "consistency across sync + thread + async APIs" cannot be delivered
while the *sync* APIs disagree this much. Pinning one design is Phase 0 work.

### Finding 3 — the spec already promises the thread-safe flavor, and no binding can honour it

`cell-model.md` § *Threading, permissions, and instrumentation*:

> **Threading contract**: `QueueCell` inherits the `Context` threading model.
> MPSC on the same `Context` uses `batch()`; **cross-thread MPSC requires
> `ThreadSafeContext`**; cross-process requires a distributed storage backend.

There is no constructor in any binding that accepts a `ThreadSafeContext`. The
sentence is unimplementable as written. This is a **spec-vs-implementation gap**,
not (yet) a spec violation — nothing in the normative clause list *requires* the
flavor; the threading paragraph merely describes how you would reach it.
Phase 0 has to decide which side moves.

### Finding 4 — the blocking primitive: atomic multi-root invalidation

The queue family's central correctness property is that one op invalidates
*exactly* the changed reader-kinds **in one atomic frontier walk**, so a
subscriber never observes `len` bumped while `is_full` has not yet flipped
(`queue.rs:495-529`, and the `invalidates` matrix every fixture asserts).

In lazily-rs that primitive is:

```
src/context.rs:2236    pub(crate) fn clear_slots(&self, ids: &[SlotId])
```

- **`Context`** — has it, but it is **crate-private**.
- **`ThreadSafeContext`** — has only single-handle `clear<T>(&Computed<T>)`
  (`thread_safe.rs:3549`). Clearing four handles in four calls is four frontier
  walks; a concurrent reader can interleave. `batch()` may or may not close this —
  **verify before assuming**, it is the single highest-risk unknown in this plan.
- **`AsyncContext`** — has **no `clear` at all**, and no synchronous `computed`;
  only `computed_async` (`async_context.rs:413`), whose current use shape wraps a
  ready future.

So the async flavor is not "the same code with a different context type". It
needs, at minimum, `AsyncContext::computed` (sync-compute, async-graph — the same
widening the ReactiveMap plan identified for `AsyncMapHandle::materialize`) and a
multi-root clear. Both are **context** work, not queue work, and must land before
any queue shell can be flavored.

### Finding 5 — what genuinely is flavor-agnostic

Everything below the reader-kind plane. `QueueStorage` / `VecDequeStorage`, the
FIFO algebra, the `TopicCell` cursor/retention/GC algebra, and the
`WorkQueueCell` lease/ack/nack/redelivery/DLQ state machine touch no handle and
await nothing — exactly the property that let ordering and atomic move bind every
ReactiveMap flavor at no language cost. The three Lean models
(`QueueCell.lean`, `TopicCell.lean`, `WorkQueueCell.lean`) pin that
storage-agnostic layer and are already flavor-neutral.

**So the split is clean:** a graph-agnostic core that already exists, plus a
per-flavor reader-kind plane that must be minted on each flavor's own graph —
structurally identical to the membership/order plane in the ReactiveMap work,
where a shared closure-free core could **not** supply reactivity.

## Phases

### Phase 0 — spec (`lazily-spec`)

Blocking; everything else derives from it.

1. **Add a Core-surface section for the queue family**, mirroring
   `cell-model.md` § *Core surface vs. binding extensions* (which already exists
   for `ReactiveMap`). State per primitive which methods are Core (REQUIRED of
   every binding × every flavor, fixture-gated) and which are Extended
   (OPTIONAL, unscored). Candidate Core per primitive:
   - `QueueCell`: `push`/`try_push`, `pop`/`try_pop`, `close`, and the reactive
     reader-kinds `head` / `len` / `is_empty` / `is_full` / `closed`.
   - `TopicCell`: `publish`, `subscribe`/`reconnect`/`disconnect`, cursor `read`
     / `advance`, retention/GC observables.
   - `WorkQueueCell`: `push`, `claim`, `ack`, `nack`, `reap_expired`, and the
     `pending_len` / `in_flight_len` / `dead_letter_len` reader-kinds.
2. **Pin one reader-kind design.** Decide between memoized-derive+invalidate (5
   bindings) and version-cell+subscribe (cpp). Recommend **memoized-derive**: it
   is what the demand-driven-derivation clause already specifies, it is the
   majority, and the version-cell form charges a graph write on every op even
   with no subscriber — the exact cost that clause exists to avoid. Then say
   explicitly that *polling a counter is not a reader-kind*, which makes zig's
   and js's current shapes non-conforming rather than silently green.
3. **Define the per-flavor obligation.** Reuse the ReactiveMap wording: a flavor
   MUST preserve the reader-kind independence and atomicity laws and adds its
   context-specific guarantee — confluence (thread-safe), eventual transparency
   (async). Name the atomicity requirement explicitly: *one op invalidates all
   changed reader-kinds in a single frontier walk*.
4. **Resolve Finding 3**: either the threading paragraph becomes normative (a
   `ThreadSafeContext`-bound flavor is REQUIRED) or it is rewritten to describe
   an aspiration. Do not leave it as prose that no binding implements.
5. **Split `coverage.json`'s three conflated rows into nine** (primitive ×
   flavor), the same way the ReactiveMap row became four.

**Open spec questions to settle here, not later:**
- Does `TopicCell` broadcast fan-out mean anything different under a
  `ThreadSafeContext`, or is a subscription cursor already confluent by
  construction? (Cursors are per-subscriber and monotone, so probably yes —
  but state it.)
- Under async, does `pop` return a value or a future? The ReactiveMap
  verification found seven of nine bindings already have a **synchronous**
  async-map read; the same is likely true here and would keep `pop` uncoloured.
  Verify before writing it down.
- `WorkQueueCell` lease expiry is time-driven (`now: u64` passed in). Under
  async, is `reap_expired` still caller-driven, or does the flavor own a timer?
  Caller-driven keeps it deterministic and fixture-replayable — recommend that.

### Phase 1 — formal (`lazily-formal`)

The three Lean models are flavor-neutral today and should stay that way. What is
missing is a theorem that *justifies* flavor-neutrality rather than assuming it:

1. Prove the reader-kind invalidation set is a pure function of
   `(op, len_before, len_after)` — already the load-bearing comment in rs, zig,
   and kt, unproven anywhere. This is what makes the plane portable across
   flavors without re-deriving values.
2. Prove atomicity: no interleaving of a single op's invalidations is observable
   to a reader that sees the post-op storage state.
3. Cross-check against `ThreadSafe.lean` for the confluence obligation; add a
   queue-family instance if one is not already derivable.

Do **not** add per-flavor Lean models — the whole point is that the algebra does
not fork.

### Phase 2 — conformance (`lazily-spec/conformance/collections/`)

The nine existing fixtures (`queuecell_*`, `topiccell_*`, `workqueue_*`) carry a
`"model"` field naming the primitive but **no execution-model axis**. That is
correct and should stay: as with the reactive-graph corpus, the flavor axis lives
in the **runner**, which replays the same corpus against each model
(zig's `Engine(comptime Model)` over `SyncModel` / `TsModel` / `AsyncModel` is
the reference shape).

1. Extend each binding's queue runner to replay the whole corpus against all
   three flavors, with a **declared skip ledger** for flavors a binding does not
   ship — modelled on lazily-zig's reactive-graph runner, which fails if
   everything is skipped and requires the skip ledger to match observation
   exactly.
2. **Add a positive assertion that fixtures actually ran** (count replayed steps,
   assert > 0 per flavor). This is the documented failure mode from the
   conformance-delivery repair: three separate ways to report green while testing
   nothing.
3. **Check the `invalidates`-nesting trap in every binding.** In lazily-rs,
   `collections_conformance.rs` read `invalidates` off the *step* while the
   fixtures nest it under `expected` — so it was always `None` and
   `assert_invalidation` never ran. The queue fixtures nest it the same way
   (`steps[].expected.invalidates`). Grep every queue runner for this before
   trusting any green result.
4. Mutation-check each runner: break the library on purpose (drop one
   invalidation, flip one bound) and confirm the suite goes red **per flavor**.

Phase 2 lands **before** any implementation work, so the gap turns from invisible
to red first.

### Phase 3 — context prerequisites + reference implementation in lazily-rs

1. **`ThreadSafeContext`**: add a multi-root `clear_slots` equivalent (or prove
   `batch()` already makes N single clears atomic — measure, do not assume).
2. **`AsyncContext`**: add a synchronous-compute `computed` and a multi-root
   clear. This is the same widening the ReactiveMap plan calls for on
   `AsyncMapHandle::materialize`; do them together if both land in the same cycle.
3. Make `Context::clear_slots` reachable by the queue shells on all three
   flavors (it is `pub(crate)` today, which is fine as long as the shells stay
   in-crate — confirm, do not widen the public API needlessly).
4. Extract the storage/algebra core if it is not already separable, then ship
   `ThreadSafeQueueCell` / `AsyncQueueCell` and the `TopicCell` /
   `WorkQueueCell` equivalents, each minting **its own** reader-kind handles on
   **its own** graph. Interior mutability moves `Rc<RefCell<S>>` →
   `Arc<Mutex<S>>` for the thread-safe flavor; run invalidation **outside** the
   storage lock, as the ReactiveMap membership/order plane does (`ffc0d83`).
5. Replay all nine fixtures against all three flavors. `cargo test
   --all-features` + `clippy -D warnings`.

rs is the reference because it is the binding where the ReactiveMap equivalent
already landed, so the shape is known and reviewable.

### Phase 4 — reactivity repair where the family is not reactive (zig, js)

Must precede Phase 5 for these two bindings.

- **zig**: make the queue family graph-backed — reader-kinds become real nodes,
  not `u64` counters. Same repair the ReactiveMap plan lists for zig's sync map;
  consider doing both in one pass since they share the finding.
- **js**: bind `QueueCell` / `TopicCell` / `WorkQueueCell` to a context and mint
  real reader-kinds. Also check whether the collections fixtures are being
  satisfied by a fourth non-reactive model here too, the way
  `test/collections.test.js` drives `src/collections.js` instead of the reactive
  families.

### Phase 5 — fan-out to the remaining bindings

One subagent per binding, **never two against the same repo**. Dispatch only
after Phases 0–3 are committed, so each agent has a settled spec, a red
per-flavor gate, and a reference diff to mirror.

Order (cheapest first, matching the ReactiveMap sequencing rationale):

1. **cpp** — `BasicThreadSafeContext` holds a `Context`; `AsyncContext` holds a
   `Context`. Also needs the reader-kind design migrated if Phase 0 picks
   memoized-derive over its version cells.
2. **dart** — `ThreadSafeContext` wraps a plain `Context`; all three families
   already take the same `Context`. Note the open question of whether dart's
   thread-safe flavor should exist at all (spec marks it N/A for single-isolate
   Dart) — settle that before extending or deleting.
3. **py** — `ctx: dict` is already type-erased, so the projection is free.
4. **go** — still v0.22.0 on an unversioned module path; break it before v1.
5. **kt** — no default type arguments, Kotlin 2.0.21 without context parameters;
   ordinary cost, not a blocker.
6. **cs** — the family does not exist at all (already tracked as its own item).
   Build it flavor-complete from the start rather than shipping sync-only and
   repeating this plan.

## Verification

Per binding, per flavor, no exceptions:

- All nine collections fixtures replay green against sync, thread-safe, and
  async — or are recorded in a declared skip ledger that the runner enforces.
- A positive assertion that the fixtures ran (step count > 0 per flavor).
- The `invalidates` matrix is actually asserted (grep for the `expected`-nesting
  trap).
- Mutation check: one deliberate defect turns each flavor red.
- Full local suite per binding + `make check` where it exists.
- `coverage.json` regenerated with the nine split rows.

## Anti-goals

- No distributed queue backend. `RaftQueueStorage` and the broker adapters stay
  the separate v1 non-goal the PRD describes.
- No `MPSCQueueCell` / `SPSCQueueCell` type split — the spec explicitly forbids
  it; MPSC is a usage rule, not a type.
- No per-flavor Lean models.
- No release/publish in any phase. Releases stay gated on the per-cell CRDT work.
