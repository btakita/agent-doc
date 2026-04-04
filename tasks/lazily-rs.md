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
- [x] Phase 5: Testing & benchmarks
- [ ] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->

### Re: Update README, audit docs, release

**README updated** with:
- "Why Lazy?" comparison table (lazy vs eager)
- API reference table (all 7 public methods)
- Multi-language implementation table (Rust/Zig/Python)
- Dynamic dependency explanation
- Context description expanded (maps to request context, app scope)
- Blog post link was already added

**SPEC.md fixed** — was significantly out of date:
- Changed `HashMap<TypeId, ...>` → `HashMap<SlotId, Node>` (actual impl)
- Changed `Slot<T>`, `Cell<T>` structs → `SlotNode`, `CellNode` (actual impl)
- Changed `&mut Context` → `&Context` (uses interior mutability)
- Changed API methods to match actual surface (`ctx.slot()`, `ctx.get()`, etc.)
- Fixed lazily-py comparison: `SlotId (u64)` not `TypeId`, `Direct context passing` not `Trait-based`
- Fixed lazily-rs thread safety: `Single-threaded (RefCell)` not `Feature flag`

**AGENTS.md updated:**
- Added `tests/spec_compliance.rs` (36 tests)
- Updated test count to 49 total
- Restored Architecture section

**Release decision:** Changes are docs-only. v0.1.1 is already published. crates.io serves the README from the last published version, so users see the v0.1.1 README until a new publish. If you want the updated README on crates.io, we'd need v0.1.2. Otherwise these changes are available on GitHub immediately.

**Blog post API discrepancy:** The blog post shows a different API pattern (`Cell::new()`, `Slot::new()`, callable `counter(&ctx)` syntax) that doesn't exist in the current implementation. The current API uses `ctx.cell()`, `ctx.slot()`, `ctx.get_cell()`, `ctx.set_cell()`. Want me to either update the blog post to match the actual API, or implement the blog post's API in the library?
<!-- agent:boundary:1b6173d1 -->
<!-- /agent:exchange -->
 
