---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->

- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [x] Phase 3: Derived/computed signals
- [x] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [x] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260610-014948.md`*

Compacted content:
- Archived 1 response topic(s): Transfer repos to lazily-hub
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: Next steps for lazily-zig spec compliance — opus

> **Queue prompt:**
>
> [#r5an]


> **Queue prompt:**
>
> What are #next-steps to fix bugs in lazily-rs?


> **Queue prompt:**
>
> What are #next-steps to make lazily-zig compliant with the spec?


The core gap is **architectural**: lazily-zig destroys slots on invalidation while the spec requires marking them dirty with preserved cached values. This blocks memo guards, effects, batching, and most downstream features.

**Priority order (each item gates the next):**

1. **`#lzinvmod`** — Refactor invalidation model to dirty-state (fresh/dirty/unset) with lazy recomputation
2. **`#lznodevec`** — Refactor storage to `Vec(?Option(Node))` with sequential SlotId + free-list
3. **`#lzmemo`** — Memo with PartialEq guard (requires dirty-state model)
4. **`#lzeffect`** — Effect system (auto-tracking, scheduling, coalescing, cleanup, disposal)
5. **`#lzbatch`** — Batch system (nested, outermost-boundary flush, deferred effects)
6. **`#lzcellguard`** — Cell PartialEq change detection on set()
7. **`#lzslotops`** — Missing slot operations: clear(), is_set(), clear_dependents(), computed alias
8. **`#lzthreadsafe`** — ThreadSafeContext with sidecars, in-flight dedup, optimistic reads
9. **`#lzasync`** — AsyncContext with async state machine and cancellation
10. **`#lzipc`** — IPC layer (Snapshot, Delta, IpcSink/Source)
10. **`#74c7` / `#lzipc`** — Add IPC last, since it depends on stable snapshots/deltas over the core model.
- Committed and pushed the superproject submodule pointer at `9f64f1d9f`.
- Pushed `lazily-rs` `main` to `origin/main` at `ed08785` (`test: lock dirty slot cache validation`).
- The superproject has no workflow named `CI`.
- lazily-rs GitHub Actions CI started after push and is currently `in_progress` for run `27314803102`.
- Full `make check` passed, including fmt, clippy, build, default/tokio/async/distributed/ffi/ipc/signaling tests, loom, doctests, and benchmark result check.
- Targeted invalidation tests passed.
- `gh run list --workflow CI --limit 1` in `src/lazily-rs`
- `make check`
- `cargo test equal_intermediate_slot_prevents_downstream_recompute --test spec_compliance`
- `cargo test invalidation_semantics --test spec_compliance`
- `cargo fmt --check`
- `src/lazily-rs` submodule pointer in the superproject
- `src/lazily-rs/tests/spec_compliance.rs`
9. **`#9evs` / `#lzasync`** — Add async context support after core invalidation/effect semantics are proven.
8. **`#6965` / `#lzthreadsafe`** — Add ThreadSafeContext after the single-threaded semantics are correct.
7. **`#p2pc` / `#lzslotops`** — Fill in missing slot API surface: clear(), is_set(), clear_dependents(), computed alias.
6. **`#39xb` / `#lzcellguard`** — Add Cell.set() equality no-op behavior.
5. **`#qhn6` / `#lzbatch`** — Add batching and deferred effect flushing on top of stable effect scheduling.
4. **`#1xk0` / `#lzeffect`** — Implement effects after memo/dependency propagation is stable.
3. **`#dtmx` / `#lzmemo`** — Add memo equality guards once dirty recomputation can preserve cached values.
2. **`#ce90` / `#lznodevec`** — Bring storage in line with the spec: sequential SlotId-backed node storage with free-list reuse.
1. **`#r5an` / `#lzinvmod`** — Fix invalidation first by preserving slots and marking them dirty instead of destroying cached state.

Items 1-2 are foundational refactors. Items 3-7 build on the new invalidation model. Items 8-10 are advanced features.

**Top of backlog: `#lzinvmod`**
<!-- agent:boundary:513b870a -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go=auto -->
- ~~What are #next-steps to fix bugs in lazily-rs?~~
- ~~[#r5an]~~
- [#ce90]
- [#dtmx]
- [#1xk0]
- [#qhn6]
- [#39xb]
- [#p2pc]
- [#6965]
- [#9evs]
- [#74c7]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#ce90] #lznodevec=Refactor node storage from HashMap(usize, *Slot) to Vec(?Option(Node)) indexed by sequential SlotId with free-list reuse per spec.
- [ ] [#dtmx] #lzmemo=Implement Memo (ctx.memo) with PartialEq guard — dirty recomputation producing equal value suppresses downstream invalidation.
- [ ] [#1xk0] #lzeffect=Implement Effect system — immediate activation, auto-tracking, scheduling, coalescing, cleanup closures, disposal, is_active.
- [ ] [#qhn6] #lzbatch=Implement Batch system — nested batches, outermost-boundary flush, coalesced updates, deferred effect scheduling.
- [ ] [#39xb] #lzcellguard=Add PartialEq change detection to Cell.set() — no-op on equal value per spec.
- [ ] [#p2pc] #lzslotops=Add missing slot operations: clear(), is_set(), cell.clear_dependents(), computed alias.
- [ ] [#6965] #lzthreadsafe=Implement ThreadSafeContext with per-slot read-mostly sidecars, in-flight dedup, optimistic reads, InvalidationPlan.
- [ ] [#9evs] #lzasync=Implement AsyncContext — async computed/memo/effect, AsyncSlotNode state machine, cancellation.
- [ ] [#74c7] #lzipc=Implement IPC layer — Snapshot, Delta, DeltaOp, IpcSink/Source, ShmBlobArena.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jpf1] #webrtc-data=WebRTC data channel transport for peer-to-peer IPC (beyond signaling)
- [/] [#w30b] #gh-org=Create GitHub org (lazily-reactive or lazily) and transfer repos
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
