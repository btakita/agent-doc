# Plan: eager `Signal` primitive for lazily-zig (`#lzsigzig`)

**Status:** Design — blocked on engine changes (deferred-recompute queue). Not a direct mirror of lazily-rs/py.
**Created:** 2026-06-24
**Session:** `tasks/software/lazily-rs.md`
**Backlog:** `#lzsigzig` (`#t0mf` / `#t0qn`)

## Goal

Add `src/lazily-zig/src/lazily/signal.zig` mirroring lazily-rs `SignalHandle<T>` (`src/lazily-rs/src/signal.rs`) and lazily-py `Signal[T]` (`src/lazily-py/src/lazily/signal.py`): an **eager** derived value — memoized slot + puller + `PartialEq` memo guard, always materialized (no unset state).

## Why this is NOT a direct port (the blocker)

lazily-zig's invalidation model differs fundamentally from its siblings:

| Engine | Invalidation | Re-borrow during recompute | Eager seam |
|---|---|---|---|
| lazily-rs | mark-dirty (RefCell) | re-borrow OK (panic, not deadlock) | `EffectHandle` puller re-runs cleanly |
| lazily-py | slot `reset()` hook | n/a (dict, no lock) | `_SignalSlot.reset` → `_eager_recompute` |
| **lazily-zig** | **destroy + free** (`destroySelf` `context.zig:354`) | **`Thread.Mutex` is NOT reentrant → deadlock** | **none today** |

Two concrete obstacles:

1. **Lock re-entrancy.** `Slot.emitChangeUnlocked` runs under `ctx.mutex`. An inline eager recompute must re-run the user `valueFn`, which calls `slot()`/`slotKeyed()` (`slot.zig:59` `ctx.mutex.lock()`) → re-acquire the same mutex → **deadlock**. lazily-rs `RefCell` and lazily-py's dict do not have this problem.
2. **Destroy-on-invalidate.** lazily-zig frees the `Slot` (`ctx.allocator.destroy(self)`, `context.zig:354`) on invalidation — a slot has no stable identity across invalidations. lazily-rs/py keep the slot alive and just mark/reset it. There is also a latent iteration-during-mutation: `emitChangeUnlocked` iterates `change_subscribers` while `destroyUnlocked` → `unsubscribeChangeUnlocked` removes entries from that same map.

## Design — deferred-recompute queue (recommended)

Add a **pending-recompute queue** on `Context`, drained *outside* the graph mutex so user `valueFn`s can re-acquire it per-op:

### Engine changes (`context.zig`)

- `Slot` gains two type-erased fn pointers (default `null`, zero impact on existing slots):
  - `on_invalidate: ?*const fn(*Slot) void` — fired instead of `destroyUnlocked` when a dependency invalidates this slot.
  - `recompute: ?*const fn(*Slot) void` — the type-erased re-materialize (re-run `valueFn`, memo guard, swap storage, re-`emitChange`).
- `emitChangeUnlocked` rewrite — snapshot dependents into an allocator slice, **clear the map first**, then for each dependent: if `on_invalidate != null` call it (enqueue + mark stale, do NOT destroy/free); else `destroyUnlocked(true)` as today. Snapshotting also fixes the latent iteration-mutation bug.
- `Context` gains `pending_recompute: std.ArrayList(*Slot)` (+ a re-entrancy guard flag).
- `on_invalidate` hook body: push `self` onto `ctx.pending_recompute` (if not already queued), set a `stale` flag. Does **not** take the lock.
- New `Context.drainPendingRecompute()`: pop each queued slot, call `slot.recompute(slot)` *outside* the mutex. `recompute` re-runs `valueFn` under a `TrackingFrame` (re-establishes dependency edges), applies `std.meta.eql` memo guard, swaps `storage` (freeing old `.indirect`), and `emitChange()`s to the slot's own dependents if the value changed (cascading eagerness). Equal recompute suppresses the cascade.

### Drain points (where the queue is flushed, lock-free)

- `Slot.emitChange()` (public, `context.zig:274`) — after `unlock()`, call `ctx.drainPendingRecompute()`.
- `cell.zig Cell.set()` — after the change-callback loop (already post-unlock), call `ctx.drainPendingRecompute()`.

### `signal.zig`

```
pub fn Signal(comptime T: type) type {
    return struct {
        ctx: *Context,
        slot: *Slot,            // backing memoized slot (NOT freed on invalidate)
        active: bool,
        // get()/dispose()/is_active() mirror lazily-rs SignalHandle<T>
    };
}
pub fn signal(comptime T, ctx, comptime valueFn: *const ValueFn(T), comptime deinit) !*Signal(T)
```

- Creates the backing slot via `slot()` (establishes dependency edges + memo cache).
- Installs comptime-generated `on_invalidate` + `recompute` fns on that slot (captures `T`).
- `get()` reads `slot.get(T)` (always materialized).
- `dispose()` clears `slot.on_invalidate`/`recompute` → reverts to lazy `Slot` semantics (matches lazily-rs `dispose`).
- `is_active()` reports whether the puller is installed.

### Wiring

- `root.zig`: export `signal`, `Signal`.
- Tests in `signal.zig`: eager recompute on dependency change (no read needed), memo guard (equal recompute suppresses cascade), dispose → lazy, cascading signal-on-signal.

## Phases

1. **Engine:** `on_invalidate`/`recompute` fields + snapshot `emitChangeUnlocked` + `pending_recompute` queue + `drainPendingRecompute` + drain points. Regression: existing slot/cell tests stay green (snapshot path is equivalent for non-hook slots).
2. **Signal:** `signal.zig` + `root.zig` exports + tests.

## Alternative considered (rejected)

- **Reentrant mutex** (`parking_lot::ReentrantMutex` equivalent) — no first-class Zig stdlib type; would require a custom reentrant lock and re-audit of every lock site. Higher blast radius than the queue.
- **Cell-callback eagerness only** — would restrict `Signal` to `Cell` dependencies (not `Slot`/other `Signal`s); unfaithful.
- **Lazy-on-read (generation check)** — not eager; fails the core contract (observers must update without a read).

## Open

- Whether `pending_recompute` should be LIFO or FIFO (cascading order) — decide at impl; LIFO likely (deepest-dependency-first).
- Allocator for the dependents snapshot in `emitChangeUnlocked`: stack buffer (cap 32) with allocator fallback, or always allocator (existing `destroySelf` already allocates/frees under the lock, so allocator-under-mutex is precedent).
