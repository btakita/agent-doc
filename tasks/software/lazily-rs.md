---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260624-060903.md`*

Compacted content:
❯ - Archived 4 response topic(s): #t0mf / #lzsigzig; queue dedup; IPC dogfood log acknowledged; 1 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4af...

### Re: compaction landed; queue stopped, DAG order intact — glm-5.2

Compaction archived 4 topics cleanly. Exchange tail is clear. Sidecar snapshot was stale (18.8 KB vs 10.4 KB doc from the compacted-away content) — reset via `agent-doc reset --from-current` so the snapshot/CRDT now matches the live file.

**Queue state:** `queue: stop` in frontmatter — halted, not draining. DAG reorder from the prior cycle is intact across both queue and backlog:

| Tier | Items | Status |
|------|-------|--------|
| 0 — no deps, unblocks | `#9226` → `#nnyn` → `#hj5x` | ready |
| 1 — dep on #9226 | `#f71y` | blocked |
| 2 — unblocked FFI consumers | `#mes4` → `#bcnf` | ready (`#lzffistate1` DONE) |
| 3 — superseded context | `#t0qn` → `#t0mf` | narrowed by #9226/#f71y split |
| 4 — operator clarification | `#pwvm`/`#9e64` · `#t1xt`/`#dcgp` · `#7gpg`/`#kjfe` | blocked |

**`:pushpin:` prefixes:** supervisor re-injected on 7 queue heads (`#qauthorder` known bug). Inert while queue is stopped; will strip on next resume.

**Semantic-completion warnings (3):** all false positives, none auto-closed — `#h6qb` is genuinely operator-gated (live two-host/NAT); `#bcnf`/`#mes4` are JS/Kotlin consumer packages, distinct from the Rust FFI exports in `#lzffistate1`.

**Next head:** `do [#9226]` — lazily-zig Signal engine (deferred-recompute queue in `context.zig`, plan `c74e75909`). Flip `queue: go` or `queue: auto` in frontmatter to resume the drain, or point me at a specific item.
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=2de2f20d-95ca-4e81-9a41-02a297107b33 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=11941 candidate_hash=cfa0bba8393931e6c79be6283ec57c348006791ed303b22f2db57348510bf1f4 content_ours_len=20372 content_ours_hash=62e4dcfd82e39c90e6415373dc870ac8675f098d15aa4114c74f358ce81ebb9c doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782281390181
```

### Re: #9226 DONE — Signal engine Phase 1 (lazily-zig context.zig) — glm-5.2

> **Queue prompt:**
>
> do [#9226]

**Implemented and committed.** `src/lazily-zig/src/lazily/context.zig` + `cell.zig` + `build.zig` (submodule commit `0c2c81a`, superproject gitlink `e69684c6e`):

- **`Slot` fields** (default `null`/`false` — zero impact on existing slots): `on_invalidate: ?*const fn(*Slot) void`, `recompute: ?*const fn(*Slot) void`, `stale: bool`.
- **`Context` queue**: `pending_recompute: std.ArrayList(*Slot)` + `draining_recompute: bool` re-entrancy guard. `drainPendingRecompute()` pops LIFO, calls `slot.recompute(slot)` **outside the mutex** — user `valueFn` can re-lock per-op without deadlock.
- **`emitChangeUnlocked` rewritten**: snapshot dependents into allocator slice → **clear map first** → iterate snapshot. Fixes the latent iteration-during-mutation bug (`destroySelf` → `unsubscribeChangeUnlocked` removed entries from the map mid-iteration). For each dependent: if `on_invalidate != null` → call hook (enqueue + mark stale, do NOT destroy); else `destroyUnlocked(true)` as before.
- **Drain points**: `Slot.emitChange()` (after `unlock`) and `Cell.set()` (after callback loop).
- **`build.zig`**: `@hasField(std.Build, "args")` guard for Zig 0.17 compat (pre-existing `b.args` API break, not Signal-related).

**Verification:** `mise exec zig@0.15.2 -- zig build test --summary all` → **28/28 tests pass**. No slots use the hooks yet (purely additive) — existing slot/cell tests exercise the snapshot path and stay green.

**Scope:** Phase 1 only (engine). Phase 2 (`#f71y`) = `signal.zig` implementing `Signal(T)` with comptime-generated `on_invalidate`/`recompute` fns + tests. `#f71y` was blocked on this item — now unblocked.

### Re: Zig 0.16 support added to lazily-zig — glm-5.2

> **Queue prompt:**
>
> update lazily-zig, add support for 0.16.0

**Implemented and committed.** Submodule `9b492d8`, superproject gitlink `045d90abe`. Installed `zig@0.16.0` via mise and fixed two API breaks:

- **`std.Thread.Mutex` removed in 0.16** → added `GraphMutex` comptime type in `context.zig`: `std.Thread.Mutex` for `< 0.16`, spinlock over `std.atomic.Mutex` for `>= 0.16`.
- **`std.heap.ThreadSafeAllocator` removed in 0.16** → guarded the thread-safe contention test in `cell.zig` (`SkipZigTest` for `>= 0.16`); `cell_0_16_test.zig` uses `std.testing.allocator` directly.
- **`ArrayList` init**: comptime `.{} ↔ .empty` (0.16 removed default field values).
- `.mise.toml`: added `test_0_16` task.

**Verification:** 28/28 pass (0.15.2), 32/33 pass (0.16.0, 1 skip = `ThreadSafeAllocator` test).

Note: `#9226` (Signal engine Phase 1) was committed in the prior cycle (`0c2c81a` / gitlink `e69684c6e`) — the response was scrambled by IPC transit but the work landed. Both commits are on `main`.

**Next drain head:** `do [#nnyn]` (StateMachine in lazily-zig) or `do [#f71y]` (signal.zig Phase 2, now unblocked).
<!-- agent:boundary:69236c47 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#9226]~~
- ~~update lazily-zig, add support for 0.16.0~~
- :pushpin: do [#nnyn]
- :pushpin: do [#hj5x]
- :pushpin: do [#f71y]
- :pushpin: do [#mes4]
- :pushpin: do [#bcnf]
- do [#t0qn]
- do [#t0mf]
- :pushpin: do [#pwvm]
- do [#9e64]
- do [#t1xt]
- do [#dcgp]
- do [#7gpg]
- do [#kjfe]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#nnyn] #lzsmzig=Implement StateMachine in lazily-zig (plan Phase 3). Add src/lazily-zig/src/lazily/state_machine.zig: StateMachine(S,E) wrapping Cell + *const fn(*const S,E)?S transition pointer (Zig has no closures). Tests for guard rejection + acceptance.
- [ ] [#hj5x] #lzsmlean2=Add Lean cross-module proof: SM send -> IPC DeltaOp.cellSet (plan Phase 1 TODO). Prove a materialized StateMachine transition is observable on the wire. Link StateMachine.lean with IPC.lean.
- [ ] [#f71y] #lzsigzig2=Implement eager Signal in src/lazily-zig/src/lazily/signal.zig (mirrors lazily-rs SignalHandle + lazily-py Signal): memoized backing slot + comptime on_invalidate/recompute puller + std.meta.eql memo guard. get()/dispose()/is_active(). Export from root.zig. Tests: eager recompute, memo-guard suppresses equal cascade, dispose->lazy, signal-on-signal. Blocked on #lzsigzig1. Plan Phase 2.
- [ ] [#mes4] #lzkt=[recommended] Implement lazily-kt FFI-consumer package (plan Phase 4). Kotlin/Gradle under src/lazily-kt/. Wraps JNA bridge to lazily_ffi_*; StateFlow<DocumentStateProjection> projection holder + recordStateEvent reporter. NOT a reactive-core port. Blocked on #lzffistate1.
- [ ] [#bcnf] #lzjs=[recommended] Implement lazily-js FFI-consumer package with *.js + *.d.ts (plan Phase 5). Under src/lazily-js/. Wraps VSCode N-API addon; EventEmitter projection holder + event reporter. NOT a reactive-core port. Blocked on #lzffistate1.
- [ ] [#t0qn] #lzsigzig=Implement Signal primitive in lazily-zig (src/lazily-zig/src/lazily/signal.zig). Zig currently has Slot+Cell but no Signal. Mirror lazily-rs eager Signal: memoized slot + puller effect + PartialEq memo guard.
- [ ] [#t0mf] #lzsigzig=Implement eager Signal in lazily-zig (src/lazily-zig/src/lazily/signal.zig). NOT a direct mirror: lazily-zig destroy-on-invalidate (destroySelf frees the Slot, context.zig:354) + non-reentrant Thread.Mutex means an inline eager recompute DEADLOCKS (user valueFn re-locks ctx.mutex via slot()/slotKeyed). lazily-rs RefCell + lazily-py dict have no such re-entrancy issue. Plan + deferred-recompute-queue design committed: tasks/software/plan-lazily-zig-signal.md (c74e75909). Phase 1 = engine changes in context.zig (on_invalidate hook + pending_recompute queue + drain points outside the lock + snapshot emitChangeUnlocked to also fix latent iteration-mutation). Phase 2 = signal.zig. Splitting #t0mf into #lzsigzig1 (engine) + #lzsigzig2 (signal.zig) below.
- [ ] [#pwvm] #lzffileanrs=[recommended] Clarify: IPC.lean protocol already implemented in lazily-rs ipc.rs (Snapshot/Delta/ShmBlobArena). If goal is ShmBlobArena parity in siblings or Lean proof extraction, scope a plan.
- [ ] [#9e64] #lzffileanrs=[recommended] Clarify: IPC.lean formal theorems already implemented in lazily-rs ipc.rs (Snapshot/Delta/DeltaOp/epoch sequencing). If goal is ShmBlobArena/ShmBlobRef in siblings or Lean-to-Rust proof extraction, scope a plan. Currently ambiguous.
- [ ] [#t1xt] #lzffileanpy=[recommended] Clarify: lazily-py ipc.py already implements IPC.lean (IpcValue/SharedBlob/Snapshot/Delta). If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#dcgp] #lzffileanpy=[recommended] Clarify: lazily-py ipc.py already implements the IPC.lean protocol (IpcValue/SharedBlob/Snapshot/Delta). If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#7gpg] #lzffileanzig=[recommended] Clarify: lazily-zig ipc.zig already implements IPC.lean. If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#kjfe] #lzffileanzig=[recommended] Clarify: lazily-zig ipc.zig already implements the IPC.lean protocol. If goal is ShmBlobArena parity or proof extraction, scope a plan.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#kb9b] #lzpypub011=[operator-drive] Publish lazily-py v0.11.0: SHIP-STOPPER #lzpysigfix is FIXED (commit 30907ea). Build artifacts rebuilt in src/lazily-py/dist/ (lazily-0.11.0-{whl,tar.gz}, twine check PASSED, fresh-venv install round-trip verified). From src/lazily-py: tag v0.11.0 + push, gh release create v0.11.0 --generate-notes, then python -m twine upload dist/* (needs PyPI token + browser/passkey 2FA). Gated on the pre-publish live-test confirm per the testing-gate rule. Commit 30907ea + superproject gitlink eb93a75ac already pushed.
- [/] [#kabc] #lipostlazilyarch=[operator-drive] Main LinkedIn post is published and repaired: Corky edit updated mail/social/20260616-lazily-architecture-linkedin.md / https://www.linkedin.com/feed/update/urn:li:share:7472666756810190848 with the full body after the public post truncated at "The core is small: three primitives". Local metadata is committed in mail commit 07a9ea1 and superproject gitlink e7e0b08c1. Remaining operator follow-up: post the first comment from mail/drafts/linkedin/2026-06-16-lazily-architecture.md via `corky linkedin comment <published-file> <comment-body>` if still desired.
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet backend through the LIVE #yxjw signaling Worker (two real hosts / NAT). Agent-doable slice (#lzwebrtcwire) is DONE: src/webrtc_signaling.rs wires SignalingClient↔Str0mNet and tests/webrtc_signaling.rs covers the localhost handshake. Remaining: live two-host/NAT run via deployed Worker — operator-only.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#lzsidecarlock] [recommended] agent-doc-repo: close the lost-update window among concurrent cycle_state writers. Deadlock analysis DONE (tasks/agent-doc/plan-cycle-state-flock.md): NO production mutator calls another (only test code) -> non-reentrant flock is safe. Design: bounded try_lock_exclusive (~5s, fs2) + RAII guard on a dedicated .json.lock sibling (state file is atomically renamed each save -> cannot lock it directly); add `let _lock = acquire_state_lock(file)?;` to each of the ~24 public mutators in cycle_state.rs. cycle_phase_rank (:1273) guards only monotonic phase, not the ~30 non-phase fields. Hardening (not live-fire). Split from #lzsidecaratomic (DONE in agent-doc 647af082 + installed).
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
