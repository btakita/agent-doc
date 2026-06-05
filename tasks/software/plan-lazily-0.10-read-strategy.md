# lazily-rs 0.10.0 — selectable read strategy + inline small-value fast path

Owner: lazily-rs (`src/lazily-rs`). Milestone: **0.10.0** (ships all phases together).

## Goal

Let `ThreadSafeContext` cached reads use the strategy that fits the workload,
resolving the #vd5v contention tradeoff (arc-swap wins at 8+ cores, regresses
uncontended). Two user directives:

1. **Runtime-selectable strategy** — both the low-concurrency (`parking_lot::RwLock`)
   and high-concurrency (`arc_swap::ArcSwapOption`) read paths are **compiled in**;
   the mode is **selected at runtime**, default **low-concurrency**.
2. **Inline small-`Copy` fast path (c)** — values that are small and `Copy` are
   stored inline behind a seqlock (wait-free, no alloc, no refcount), optimal in
   both modes; the runtime mode only governs the large/non-`Copy` fallback.

## Phase 1 — runtime-selectable strategy (`#rdstrat1`)

- `pub enum ReadStrategy { LowConcurrency, HighConcurrency }`, `Default = LowConcurrency`.
- `ThreadSafeContext::with_read_strategy(ReadStrategy)` + `read_strategy()` getter;
  `new()`/`default()` = `LowConcurrency`. Strategy is fixed for the context's life
  (mid-life switching needs storage migration under live readers — deferred, see
  Open questions).
- Slot read sidecar becomes an enum:
  ```rust
  enum CachedReadStorage {
      Locked(RwLock<Option<Arc<ThreadSafeAny>>>),    // low-concurrency
      LockFree(ArcSwapOption<Arc<ThreadSafeAny>>),   // high-concurrency
  }
  ```
  chosen per the context strategy at slot creation. `read_fresh`/`store_value`/
  `clear` match the variant. The atomic `cache_revision` validation envelope is
  identical for both, so correctness is unchanged.
- Cost the user accepts: one per-read enum branch (both paths compiled).
- **Default flips from 0.9.0** (which shipped arc-swap as default) to RwLock →
  this is the 0.10.0 minor bump; `arc-swap` stays a normal (non-feature) dep
  since both paths compile.
- Tests: existing suite under default (Low) + a HighConcurrency variant test;
  loom model unchanged (same algorithm). SPEC: revise *Lock strategy evaluation*.

## Phase 2 — inline small-`Copy` seqlock (c) (`#rdstrat2`) — IMPLEMENTED

**Status (2026-06-05):** implemented in `src/thread_safe.rs`; loom-gated; opt-in
via new `slot_copy` / `computed_copy` / `memo_copy` constructors. See SPEC →
*Inline small-`Copy` seqlock (#rdstrat2)*.

**Design deviation (forced by stable Rust):** selection is **not** automatic on
the generic `slot`/`computed`/`memo`. Stable Rust cannot branch on `T: Copy`
inside a generic fn lacking the `Copy` bound — method resolution is
pre-monomorphization, so a `Copy`-gated impl is never *applicable* where the
bound is unprovable (an inherent-vs-trait specialization probe silently resolves
to the non-`Copy` fallback; verified). Automatic detection would need nightly
`specialization`. Resolution: opt-in `Copy`-bounded `*_copy` constructors that
compute the `InlineSpec` and route to `slot_with_equals_inline`, falling back
transparently to the strategy path when the value exceeds `INLINE_CAP` (24
bytes) or alignment 16.

**Soundness deviation:** the buffer is `[AtomicU8; INLINE_CAP]` read/written with
relaxed per-byte atomics, **not** a non-atomic `memcpy` (a non-atomic seqlock
buffer racing the writer is a formally-UB data race). Per-byte atomics make the
race well-defined and loom-modelable; `seq` Release/Acquire + an Acquire fence
give the consistency envelope (crossbeam-style). Loom models:
`inline_seqlock_reader_never_observes_torn_value`,
`inline_seqlock_envelope_rejects_torn_and_stale_under_concurrent_publish`,
`inline_seqlock_read_after_completed_invalidation_is_rejected`.

Original plan (for reference):

- Add `Inline` variant: fixed inline byte buffer + even/odd seqlock; chosen at
  creation when `size_of::<T>() <= INLINE_CAP && T: Copy`.
- Sound only for `Copy` within the size bound: a torn read is discarded on the
  seqlock retry with no `Drop`/alloc/interior-pointer deref. Large/non-`Copy`
  values use the Phase 1 `Locked`/`LockFree` fallback per mode.
- Store path: capture `(size, is_copy)` at slot creation (alongside the inline
  `TypeId`); on `store_value`, memcpy `size` bytes from the erased value into the
  inline buffer under the seqlock; reads memcpy out + validate seq, then
  `TypeId`-checked transmute to `&T`.
- **Requires a Loom/Shuttle model** of the inline seqlock (writer odd/even,
  reader retry, interplay with `cache_revision`) before it can land — per the
  SPEC lock-strategy gate.
- This realizes the SPEC's anticipated `ErasedValue` inline storage.

## Phase 3 — release 0.10.0 (`#rdstrat3`)

- Bump 0.9.0 → 0.10.0, VERSIONS/changelog note (default-strategy change),
  `make check`, tag, `cargo publish`, GitHub release. Gate on user confirmation
  (irreversible publish).

## Open questions / deferred

- Mid-life dynamic switching (changing a live context's mode) needs migrating
  every slot's storage under concurrent readers — a barrier problem. Deferred;
  Phase 1 fixes the mode at construction.
- Auto-adaptive (contention-detecting) mode — separate from explicit runtime
  selection; not in 0.10.0.
- `INLINE_CAP` value (e.g. 16/24 bytes) — tune in Phase 2.
