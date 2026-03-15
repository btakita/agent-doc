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
- [ ] Phase 2: Core signal primitives
- [ ] Phase 3: Derived/computed signals
- [ ] Phase 4: Effect system
- [ ] Phase 5: Testing & benchmarks
- [ ] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
## Evaluation

### Submodules Added

- `src/lazily-zig` — cloned from btakita/lazily-zig
- `src/lazily-py` — cloned from btakita/lazily-py
- `src/lazily-rs` — created new repo at github.com/btakita/lazily-rs and added as submodule

### SPEC.md Files Created

Created `SPEC.md` in all three submodules:
- `src/lazily-zig/SPEC.md` — Full API surface, storage modes, FFI support, thread safety
- `src/lazily-py/SPEC.md` — Context-as-dict model, slot_stack tracking, type system
- `src/lazily-rs/SPEC.md` — Proposed design bridging zig and py approaches

### Your Description vs Actual Implementations

Your description is accurate with some nuances:

**"Slots can be unset or set with a value returned by its activating function"** — Correct in both implementations. In lazily-zig, an unset slot has `storage: null`. In lazily-py, an unset slot simply has no entry in the context dict.

**"The Slot value will not be updated except to clear + unset the Slot"** — Correct. Neither implementation allows in-place mutation of a Slot's cached value. The only way to change it is `clear/destroy/reset` → recompute.

The Slot and Cell lazily recompute...meaning recomputation & subscriber notification only occurs when called again. A "Signal" would eagerly recompute...meaning recomputation & subscriber notification occurs on invalidation.

**"The Slot value may be a mutable struct/object or another immutable value"** — Correct. lazily-zig handles this via storage modes (`.direct` for pointers, `.indirect` for structs). lazily-py stores any Python object.

**"Slots can have dependencies on other Slots. If a dependency Slot clears, the dependent Slots will also clear."** — Mostly correct, but the direction terminology is inverted from the implementation. In both codebases, if Slot B depends on Slot A (B reads A during computation), then when A changes/clears, B is invalidated. In lazily-zig this uses `emitChange()` which destroys dependents. In lazily-py this uses `reset()` which cascades via subscribers.

**"Cells have a mutable value which can be updated via set and accessed via get"** — Correct in both implementations. Additionally, both implementations only trigger invalidation when the value actually changes (lazily-py checks `!=`, lazily-zig always emits).

**One correction:** lazily-zig's Cell always emits change on `set()` regardless of value equality — it doesn't compare old vs new. lazily-py does check `self._value != _value` before triggering. This is a behavioral difference worth noting for lazily-rs design.

lazily-zig should follow the same behavior as lazily-py.

### Architecture Differences

| Aspect | lazily-zig | lazily-py |
|--------|-----------|-----------|
| Context | Typed struct with allocator | Plain dict |
| Tracking | Thread-local `TrackingFrame` stack | Global `slot_stack` list |
| Slot keys | Function address (`usize`) | Object identity (self as dict key) |
| Cleanup | Explicit `destroy(recurse)` | GC + `reset()` |
| FFI | Built-in `StringView`, `@export` | N/A |
| Thread safety | Mutex (configurable) | Not thread-safe |

### Next Steps

Ready to start implementing lazily-rs. Shall I proceed with Phase 2 (core signal primitives)?

Yes...note my edit above.
<!-- /agent:exchange -->
