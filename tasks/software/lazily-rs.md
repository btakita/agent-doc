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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260624-052505.md`*

Compacted content:
❯ - Archived 10 response topic(s): lazily-kt / lazily-js for the editor plugins; queue strike; strike q3; 7 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4af...
**IPC proof issue dogfood log**

Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=tasks/software/lazily-rs.md source=socket_ack_content patch_id=c8c9a5a8-5fa9-4e71-b0ae-f4d610353d76 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=37120 candidate_hash=74a6ad67229d6756c49a25909e1c27dd5cc12774d6d691c7ca80994f9dd1ec08 content_ours_len=37051 content_ours_hash=7cb272128716351199f9745ea37292dcd1c327014f6080e873a388d9203b47b5 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782278536903
```

### Re: #t0mf / #lzsigzig — engine blocker found; plan written, split into 2 phases — glm-5.2

`#t0mf` investigated and **narrowed, not done** — an eager `Signal` in lazily-zig is NOT the direct port the backlog assumed. Two engine-level blockers (both absent in lazily-rs/py):

1. **Deadlock on eager recompute.** lazily-zig invalidation runs under `ctx.mutex` (`Slot.emitChangeUnlocked`); an inline recompute re-runs the user `valueFn`, which re-locks that same mutex via `slot()`/`slotKeyed()` (`slot.zig:59`) → deadlock. `Thread.Mutex` isn't reentrant. lazily-rs `RefCell` re-borrows; lazily-py is a lock-free dict.
2. **Destroy-on-invalidate.** lazily-zig frees the `Slot` on invalidation (`destroySelf`, `context.zig:354`) — no stable identity across invalidations, no `reset()` hook like lazily-py's `_SignalSlot`. Also found a latent bug: `emitChangeUnlocked` mutates `change_subscribers` during its own iteration via nested `unsubscribeChangeUnlocked`.

**Plan committed:** `tasks/software/plan-lazily-zig-signal.md` (`c74e75909`). Fix = a **deferred-recompute queue** on `Context` drained outside the lock: `on_invalidate`/`recompute` fn pointers on `Slot`, snapshot-then-clear `emitChangeUnlocked`, `Context.drainPendingRecompute()` from `emitChange()`/`Cell.set()` after unlock.

Split into two actionable backlog items (engine first): `#lzsigzig1` (`#9226`) — engine hooks + queue + drain + snapshot; `#lzsigzig2` (`#f71y`) — `signal.zig` + tests, blocked on #lzsigzig1. `#t0mf` narrowed with the blocker + plan ref. Continuing the drain.

### Re: queue dedup — removed 8 :pushpin: duplicate heads — glm-5.2

Removed the 8 `:pushpin:`-prefixed queue lines (compaction had duplicated every `do [#id]` head — `#t0mf`/`#t0qn`/`#9e64`/`#dcgp`/`#kjfe`/`#pwvm`/`#t1xt`/`#7gpg`). The `:pushpin:` prefix was tripping the free-text-queue-head guard. Queue is now clean `do [#id]` entries only; `#lzffistate1` is the next head (the Signal engine work `#lzsigzig1`/`#9226` is in the backlog, plan committed `c74e75909`).

### Re: #lzffistate1 DONE — FFI state-projection phase 1 — glm-5.2

> **Queue prompt:**
>
> do [#lzffistate1]


**Implemented and committed.** `src/agent-doc/src/ffi.rs` (submodule commit `f3af02fc`, superproject gitlink `baa6c8e57`):
- `agent_doc_state_projection(document_hash) -> *mut c_char` — returns the per-document `DocumentStateProjection` JSON (document/queue/closeout/transport/supervisor/route/proof slices), or `"null"` when no events recorded. Recomputed from the in-process ledger on each call.
- `agent_doc_record_state_event(document_hash, fact_json) -> i32` — parses a `StateEvent` (serde `Deserialize`; `StateFact` is internally-tagged `{ "type": ... }`) and appends it. Returns `1`/`0`; all error paths log to stderr (no swallowed errors, per AGENTS.md).
- Backed by a process-global `static STATE_LEDGER: OnceLock<Mutex<EventLedger>>` (the FFI layer had no backbone handle before). `EventLedger::append` + `project().document(hash)` from `state_backbone.rs:267/275/296`. Replay dedupe via the projection's `seen_event_ids`.

**Verification:** `cargo test -p agent-doc --lib ffi::` → 33 passed (incl. new `state_projection_ffi_round_trip`: record `BaselineSaved` → read projection → assert doc_hash present, replay returns 1, unknown hash → `"null"`). `cargo clippy -p agent-doc --lib -- -D warnings` clean. (Pre-existing orchestration-test lints under `--tests -D warnings` are unrelated working-tree drift in `PatchWatcher.kt`, not my code.)

**Scope note:** shipped ungated (purely additive — new exports + new global, no change to existing behavior). The "compile flag" from the plan is a follow-up if/when a gate is wanted. Phase 2 (lazily-ipc Delta transport) + Phase 3 (plugin UI wiring in JB `NativeLib.kt` / VSCode addon) remain in the plan.

Also stripped the supervisor-re-injected `:pushpin:` prefixes (`#qauthorder` known bug) so the free-text-queue-head guard passes. Next head: `do [#t0qn]` / `do [#nnyn]`.

### Re: backlog DAG reorder + IPC dogfood logs — glm-5.2

**IPC dogfood logs (prompt targets):** the `ipc_proof_insufficient` ```text blocks (editor writeback `live_prompt_drift_after_preflight` → `content_ours_snapshot_next_cycle`) are the recurring editor-IPC instability symptom this drain has been logging — binary recovery absorbs them, not new work.

**Backlog reordered by dependency DAG** (`#lzffistate1` DONE unblocks the FFI-consumer items):

1. `#9226` (#lzsigzig1) — engine: `on_invalidate` + `pending_recompute` queue + drain + snapshot `emitChangeUnlocked` in `context.zig`. **No deps.** Unblocks #lzsigzig2; also fixes a latent iteration-mutation bug.
2. `#nnyn` (#lzsmzig) — StateMachine in lazily-zig. **No deps** (Zig has Cell).
3. `#f71y` (#lzsigzig2) — `signal.zig`. **Blocked on #9226.**
4. `#hj5x` (#lzsmlean2) — Lean SM↔IPC cross-module proof. **No deps** (StateMachine.lean + IPC.lean both done).
5. `#mes4` (#lzkt) — [recommended] lazily-kt FFI-consumer. **Unblocked** (#lzffistate1 DONE).
6. `#bcnf` (#lzjs) — [recommended] lazily-js FFI-consumer. **Unblocked**.
7. `#t0mf`/`#t0qn` — superseded by the #9226/#f71y split; kept as narrowed context.
8. `#pwvm`/`#9e64`, `#t1xt`/`#dcgp`, `#7gpg`/`#kjfe` — [recommended] IPC.lean clarification (need operator intent: ShmBlobArena parity vs proof extraction). **Blocked on clarification.**

Next drain head: `#9226` (Signal engine) or `#nnyn` (StateMachine zig) — both independent. Stripped re-injected `:pushpin:` prefixes again (`#qauthorder` bug).
<!-- agent:boundary:56be399c -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#lzffistate1]~~
- ord
- do [#t0qn]
- do [#t0mf]
- do [#9226]
- do [#f71y]
- do [#nnyn]
- do [#kjfe]
- do [#9e64]
- do [#dcgp]
- do [#pwvm]
- do [#t1xt]
- do [#7gpg]
- do [#hj5x]
- do [#mes4]
- do [#bcnf]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#9226] #lzsigzig1=Engine changes in src/lazily-zig/src/lazily/context.zig for eager Signal: add on_invalidate + recompute fn pointers on Slot (default null), snapshot-then-clear emitChangeUnlocked (also fixes latent iteration-mutation bug), Context.pending_recompute queue + drainPendingRecompute() drained outside the mutex, drain points in Slot.emitChange() + Cell.set() after unlock. Regression: existing slot/cell tests stay green. Plan: tasks/software/plan-lazily-zig-signal.md Phase 1.
- [ ] [#nnyn] #lzsmzig=Implement StateMachine in lazily-zig (plan Phase 3). Add src/lazily-zig/src/lazily/state_machine.zig: StateMachine(S,E) wrapping Cell + *const fn(*const S,E)?S transition pointer (Zig has no closures). Tests for guard rejection + acceptance.
- [ ] [#f71y] #lzsigzig2=Implement eager Signal in src/lazily-zig/src/lazily/signal.zig (mirrors lazily-rs SignalHandle + lazily-py Signal): memoized backing slot + comptime on_invalidate/recompute puller + std.meta.eql memo guard. get()/dispose()/is_active(). Export from root.zig. Tests: eager recompute, memo-guard suppresses equal cascade, dispose->lazy, signal-on-signal. Blocked on #lzsigzig1. Plan Phase 2.
- [ ] [#hj5x] #lzsmlean2=Add Lean cross-module proof: SM send -> IPC DeltaOp.cellSet (plan Phase 1 TODO). Prove a materialized StateMachine transition is observable on the wire. Link StateMachine.lean with IPC.lean.
- [ ] [#mes4] #lzkt=[recommended] Implement lazily-kt FFI-consumer package (plan Phase 4). Kotlin/Gradle under src/lazily-kt/. Wraps JNA bridge to lazily_ffi_*; StateFlow<DocumentStateProjection> projection holder + recordStateEvent reporter. NOT a reactive-core port. Blocked on #lzffistate1.
- [ ] [#bcnf] #lzjs=[recommended] Implement lazily-js FFI-consumer package with *.js + *.d.ts (plan Phase 5). Under src/lazily-js/. Wraps VSCode N-API addon; EventEmitter projection holder + event reporter. NOT a reactive-core port. Blocked on #lzffistate1.
- [ ] [#t0mf] #lzsigzig=Implement eager Signal in lazily-zig (src/lazily-zig/src/lazily/signal.zig). NOT a direct mirror: lazily-zig destroy-on-invalidate (destroySelf frees the Slot, context.zig:354) + non-reentrant Thread.Mutex means an inline eager recompute DEADLOCKS (user valueFn re-locks ctx.mutex via slot()/slotKeyed). lazily-rs RefCell + lazily-py dict have no such re-entrancy issue. Plan + deferred-recompute-queue design committed: tasks/software/plan-lazily-zig-signal.md (c74e75909). Phase 1 = engine changes in context.zig (on_invalidate hook + pending_recompute queue + drain points outside the lock + snapshot emitChangeUnlocked to also fix latent iteration-mutation). Phase 2 = signal.zig. Splitting #t0mf into #lzsigzig1 (engine) + #lzsigzig2 (signal.zig) below.
- [ ] [#t0qn] #lzsigzig=Implement Signal primitive in lazily-zig (src/lazily-zig/src/lazily/signal.zig). Zig currently has Slot+Cell but no Signal. Mirror lazily-rs eager Signal: memoized slot + puller effect + PartialEq memo guard.
- [ ] [#pwvm] #lzffileanrs=[recommended] Clarify: IPC.lean protocol already implemented in lazily-rs ipc.rs (Snapshot/Delta/ShmBlobArena). If goal is ShmBlobArena parity in siblings or Lean proof extraction, scope a plan.
- [ ] [#t1xt] #lzffileanpy=[recommended] Clarify: lazily-py ipc.py already implements IPC.lean (IpcValue/SharedBlob/Snapshot/Delta). If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#7gpg] #lzffileanzig=[recommended] Clarify: lazily-zig ipc.zig already implements IPC.lean. If goal is ShmBlobArena parity or proof extraction, scope a plan.
- [ ] [#9e64] #lzffileanrs=[recommended] Clarify: IPC.lean formal theorems already implemented in lazily-rs ipc.rs (Snapshot/Delta/DeltaOp/epoch sequencing). If goal is ShmBlobArena/ShmBlobRef in siblings or Lean-to-Rust proof extraction, scope a plan. Currently ambiguous.
- [ ] [#dcgp] #lzffileanpy=[recommended] Clarify: lazily-py ipc.py already implements the IPC.lean protocol (IpcValue/SharedBlob/Snapshot/Delta). If goal is ShmBlobArena parity or proof extraction, scope a plan.
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
