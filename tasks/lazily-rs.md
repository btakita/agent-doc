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

### Re: Thread Safety

No — lazily-rs is currently **single-threaded only**. It uses `RefCell` for interior mutability, so `Context` is neither `Send` nor `Sync`.

The thread-local `TRACKING_STACK` is inherently per-thread (which is correct), but the `Context` itself cannot be shared across threads.

**To add thread safety** (future work):
- Replace `RefCell<Inner>` with `RwLock<Inner>` or `Mutex<Inner>` behind a `thread-safe` cargo feature flag
- `Context` would implement `Send + Sync` when enabled
- lazily-zig uses mutex by default with a build flag to disable; lazily-rs could do the inverse (single-threaded default, opt-in thread safety via `features = ["thread-safe"]`)

This is a natural Phase 3/4 enhancement. Want me to implement it now, or continue with derived/computed signals first?

Please release lazily-rs with the README
<!-- /agent:exchange -->
