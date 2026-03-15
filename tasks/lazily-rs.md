---
agent_doc_session: ef27a48e-f17b-4f31-841a-30fcebcd692d
agent: claude
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->
- [ ] Phase 1: Research & design
- [ ] Phase 2: Core signal primitives
- [ ] Phase 3: Derived/computed signals
- [ ] Phase 4: Effect system
- [ ] Phase 5: Testing & benchmarks
- [ ] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
## Notes

Reactive signals library for Rust. Lazy evaluation, fine-grained reactivity, zero-cost abstractions where possible.

Counterpart to btakita/lazily-py and btakita/lazily-zig. Would integrate into agent-doc for reactive state tracking (checkbox progress, document field updates via signals/slots).

Use btakita/lazily-zig and btakita/lazily-py as inspiration for this project.

The Slot and Cell are the primary implemented data types.
Slots can be unset or set with a value returned by its activating function.
The Slot value will not be updated except to clear + unset the Slot.
The Slot value may be a mutable struct/object or another immutable value.

Slots can have dependencies on other Slots.
If a dependency Slot clears, the dependent Slots will also clear.

Cells have a mutable value which can updated via set and accessed via get.

Please add btakita/lazily-zig, btakita/lazily-py, and btakita/lazily-rs as submodules to agent-loop.
Create SPEC.md files for all 3 projects.
Evaluate my account above with the actual implementations.
<!-- /agent:exchange -->
