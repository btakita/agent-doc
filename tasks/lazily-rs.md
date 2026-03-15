---
agent_doc_session: ef27a48e-f17b-4f31-841a-30fcebcd692d
agent: claude
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->
- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [ ] Phase 3: Derived/computed signals
- [ ] Phase 4: Effect system
- [ ] Phase 5: Testing & benchmarks
- [ ] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->

### Re: Lazy vs Eager Semantics

Good distinction. Updated the SPEC.md and implementation accordingly:
- **Slot/Cell** = lazy invalidation. On dependency change, dependents are *cleared* but not recomputed until next `get()` access
- **Signal** (not implemented) = eager. Would recompute immediately on invalidation

This is reflected in the lazily-rs test `lazy_recomputation` which verifies that after `cell.set()`, the dependent slot is cleared but not recomputed until `ctx.get()` is called.

### Re: lazily-zig Cell equality check

Fixed. `Cell.set()` in lazily-zig now uses `std.meta.eql(self.value, new_value)` to skip emission when the value hasn't changed. All existing tests pass (they always set different values).

### Phase 2: lazily-rs Core Primitives — Complete

Initialized the Rust project with 13 passing tests. Structure:

```
src/lazily-rs/
├── Cargo.toml          # crate: lazily, edition 2024
├── src/
│   ├── lib.rs          # re-exports Context, SlotHandle, CellHandle
│   ├── context.rs      # Context + dependency graph + tracking stack
│   ├── slot.rs         # SlotHandle<T> (lightweight Copy id)
│   └── cell.rs         # CellHandle<T> (lightweight Copy id)
└── tests/
    └── integration.rs  # 13 tests
```

**API:**

```rust
let ctx = Context::new();
let counter = ctx.cell(0i32);
let doubled = ctx.slot(|ctx| {
    let val = ctx.get_cell(&counter);
    val * 2
});

assert_eq!(ctx.get(&doubled), 0);
ctx.set_cell(&counter, 5);
// doubled is cleared but NOT recomputed (lazy)
assert_eq!(ctx.get(&doubled), 10); // recomputed here
```

**Key design decisions:**
- `RefCell` interior mutability (single-threaded default)
- Thread-local `TRACKING_STACK` for automatic dependency discovery
- `PartialEq` guard on `Cell.set()` — same value = no invalidation
- Dependencies re-discovered on each recomputation (dynamic dependency graphs)
- `SlotHandle<T>` / `CellHandle<T>` are `Copy` — lightweight u64 ids into the context

Let's commit and push. Release as well.
<!-- /agent:exchange -->
