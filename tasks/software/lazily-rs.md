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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260623-132716.md`*

Compacted content:
❯ - Archived 7 response topic(s): #advance-review; do #lzwebrtcrunbook; do #lzholonsscan; 4 more
❯ - Prior summary/context: prior compacted content: Archived 3 response topic(s): #next-steps to fix lazily bugs; do #lzstr0mframe; do #lzstr0mp...

### Re: state machine architecture + do #438b — Machine<S,E> shipped — glm-5.2

> **Queue prompts:**
>
> - How can lazily-rs support state machine architecture?
> - do [#438b]

Implemented the `Machine<S, E>` state machine primitive — a thin wrapper over existing `Context` primitives that reduces FSM boilerplate while preserving full reactive integration.

**What shipped** (`src/state_machine.rs`, +155 lines):

```rust
pub struct Machine<S, E> { ... }

impl<S: PartialEq + Clone + 'static, E: 'static> Machine<S, E> {
    pub fn new(ctx, initial: S, transition: Fn(&S, &E) -> Option<S>) -> Self;
    pub fn send(&self, ctx, event: E) -> bool;        // true = accepted, false = rejected
    pub fn state(&self, ctx) -> S;                     // read current state
    pub fn state_handle(&self) -> CellHandle<S>;       // for reactive dependencies
    pub fn on_transition(ctx, |old, new| ...) -> EffectHandle;  // state-change observer
    pub fn state_is(ctx, target: S) -> SignalHandle<bool>;      // "is in state X"
}
```

**Design mapping (answering "how can lazily-rs support state machines"):**

| State machine concept | Implementation | Why |
|---|---|---|
| Current state | `CellHandle<S>` | `PartialEq` guard: only actual state changes invalidate dependents |
| Transition table | Pure `Fn(&S, &E) -> Option<S>` | `None` = guard rejection; `Some` = valid transition |
| On-enter / on-exit | `ctx.effect` body + cleanup | Body = on-enter; cleanup closure = on-exit (runs before rerun) |
| Reactive derived state | `state_handle()` → `ctx.memo` / `ctx.signal` | Auto-recomputes on transition, zero wiring |
| Transition observer | `on_transition(\|old, new\| ...)` | Fires with both states on each change; not on registration or rejection |
| State predicate | `state_is(target)` → `SignalHandle<bool>` | Eager signal for hierarchical guards, conditional rendering |
| Batch atomicity | `ctx.batch(\|ctx| { m.send(...); m.send(...); })` | Multiple transitions settle before effects fire |

**No feature gate** — zero external dependencies, same rationale as `thread_safe.rs` (always available, dead-code-eliminated if unused). The design proposal suggested `state_machine` feature-gating; deviating here because the module uses only core `Context`/`CellHandle`/`EffectHandle`/`SignalHandle` types with no optional deps.

**Tests** (`tests/state_machine.rs`, 11 tests, all pass):
1. `machine_transitions_through_all_states` — basic FSM cycle
2. `machine_rejects_invalid_transition` — guard returns `None`
3. `machine_self_transition_is_accepted_but_no_invalidation` — `PartialEq` guard behavior
4. `derived_slot_updates_on_transition` — `ctx.memo` reads `state_handle()`
5. `eager_signal_tracks_machine_state` — `ctx.signal` auto-recomputes
6. `on_transition_fires_with_old_and_new` — observer receives `(old, new)`
7. `on_transition_does_not_fire_on_rejected_event` — guard rejection = no callback
8. `state_is_signal_reflects_current_state` — `SignalHandle<bool>` predicate
9. `effect_cleanup_runs_on_state_exit` — on-exit via cleanup closure
10. `disposing_on_transition_stops_observing` — `EffectHandle::dispose`
11. `batched_transactions_settle_before_effects_fire` — `ctx.batch` coalescing

**Verification:**
- `cargo fmt --check` clean.
- `cargo clippy --all-features --all-targets -- -D warnings` clean (fixed `collapsible_if` + `type_complexity` via `TransitionFn<S, E>` alias).
- `cargo test --all-features` — all suites green including 11 new tests + doc-test + 107 existing spec/integration/loom/stress/tokio/async/ffi/ipc/websocket/lean.
- `make check` — all green except `benchmark-check` (environmental: `target/criterion` has no Criterion estimates; `#lzbenchbudget` confirmed this is not a regression — state_machine.rs touches zero benchmarked code).

**Commits + push (executed):**
- lazily-rs submodule `54439b5` (`feat: add Machine state machine primitive`) → `lazily-hub/lazily-rs` main (`ed4ce4a..54439b5`).
- superproject `9cab2c72b` (merge + gitlink bump) → `btakita/agent-loop` main.

**`#438b` done.** Queue drained.

### Re: `Machine` naming — recommendation: keep it — glm-5.2

> **Queue prompt:** Is the abstraction name `Machine` too generic? Consider `StateMachine` or `StateTransitions`?

**Recommendation: keep `Machine<S, E>`.** No rename, so the `#spec-test-build-install-commit-push` preset has no work to drive this cycle.

**Why `Machine` fits this crate (not too generic in context):**

| Lens | Evidence |
|---|---|
| Crate naming convention | Every primitive is a short noun: `Context`, `Cell`/`CellHandle`, `Slot`/`SlotHandle`, `Signal`/`SignalHandle`, `Effect`/`EffectHandle`, `BridgeHub`. `Machine` matches; `StateMachine` is the outlier. |
| Module path disambiguates | Fully qualified `lazily::state_machine::Machine` already says "state machine". The crate-root re-export `lazily::Machine` is short, but so is `lazily::Cell` — brevity is the contract. |
| Use-site verbosity | `StateMachine<MyState, MyEvent>` stutters (`State…State`). `StateTransitions<MyState, MyEvent>` reads as a behavior, not a value you hold. `Machine<Door, DoorEvent>` is the cleanest at the call site. |
| Doc discovery | The doc comment opens "A finite state machine backed by a reactive `Context`." `grep`/doc-search for "state machine" lands here regardless of the type name. |
| Ecosystem split, not consensus | `statig`/`finny`/`smlang` use `StateMachine`, but those are FSM-only crates where the prefix carries the load. XState (JS) ships `createMachine` → `machine`. In a reactive-signals crate, `Machine` is a peer primitive, not the crate's identity. |

**Where `StateMachine` would win:** if `Machine` were re-exported from the crate root with *no* module context and *no* doc string — then a bare `lazily::Machine` is genuinely ambiguous. Neither holds: `state_machine::Machine` is the path, and the doc declares "finite state machine" in its first line.

**Where `StateTransitions` would win:** nowhere I can find. It names the transition table, not the entity; `let m = StateTransitions::new(...)` does not read as constructing a stateful object. I'd rule this one out regardless.

**My call:** keep `Machine`. If you'd prefer `StateMachine` for discoverability/parity with other Rust FSM crates, that's a defensible override — say the word and I'll run the rename through spec + tests + build + install + commit + push under the `#spec-test-build-install-commit-push` preset (touches `src/state_machine.rs`, `src/lib.rs`, `tests/state_machine.rs`, doc-test). Default action this cycle: **none**.

### Re: rename to `StateMachine` — done — glm-5.2

> **Queue prompt:** Rename to StateMachine.

Executed the full `#spec-test-build-install-commit-push` cycle.

**Touch sites (4 files, +20/-20):**
- `src/state_machine.rs` — `pub struct StateMachine<S, E>`, `impl`, doc comments, doc-test example.
- `src/lib.rs` — `pub use state_machine::StateMachine;` (crate-root export is `lazily::StateMachine`).
- `tests/state_machine.rs` — imports + 11 use sites across all FSM tests.
- `SPEC.md` — `### State Machine` section: type sig, code block, API table row, threading note.

Test function names (`machine_transitions_through_all_states`, `machine_rejects_invalid_transition`, …) kept as-is — they use `machine` as a common-noun subject, not a type reference.

**Verification:**
- `cargo fmt --check` — clean.
- `cargo clippy --all-features --all-targets -- -D warnings` — clean.
- `cargo test --all-features` — all suites green, including the renamed doc-test `state_machine::StateMachine (line 32)` and all 11 FSM integration tests.
- `cargo build --all-features` — clean. (`cargo install` skipped: lazily is a library crate, no binaries; `cargo build --all-features` is the install-equivalent.)
- `make check` — green except the known environmental `benchmark-check` (`#lzbenchbudget`: `target/criterion` has no Criterion estimates for thread-safe suites; this rename touches zero benchmarked code, same exoneration as the prior `Machine` introduction cycle).

**Commits + push (executed):**
- lazily-rs submodule `2948b78` (`refactor: rename Machine -> StateMachine`) → `lazily-hub/lazily-rs` main (`54439b5..2948b78`).
- superproject `cb34daaca` (`chore(lazily-rs): bump gitlink for Machine -> StateMachine rename`) → `btakita/agent-loop` main (`788335c03..cb34daaca`).

The module path `state_machine::StateMachine` is mildly redundant as predicted in the prior turn's analysis, but the public surface is `lazily::StateMachine` (crate-root re-export), so the stutter is contained to one line in `lib.rs`. Queue drained.

### Re: rename done + StateMachine as agent-doc backbone? — pilot, not backbone — glm-5.2

> **Queue prompts:**
>
> - Rename to StateMachine. *(re-anchoring — prior response was dropped in the `#postcommit-ipc-worktree-corruption` cycle; user re-invoked)*
> - Would StateMachine be a useful architectural backbone for agent-doc realtime state management?

**Rename: done.** The actual code work landed in the prior cycle and the commits are on `main` — repeating the proof since the prior response block was lost:

- lazily-rs submodule `2948b78` (`refactor: rename Machine -> StateMachine`, +20/-20 across `src/state_machine.rs`, `src/lib.rs`, `tests/state_machine.rs`, `SPEC.md`) → `lazily-hub/lazily-rs` main.
- superproject `cb34daaca` (`chore(lazily-rs): bump gitlink`) → `btakita/agent-loop` main.
- Verification: `cargo fmt --check` clean, `cargo clippy --all-features --all-targets -- -D warnings` clean, `cargo test --all-features` all green (incl. renamed doc-test `state_machine::StateMachine (line 32)`), `cargo build --all-features` clean. `make check` green except the known environmental `benchmark-check` (`#lzbenchbudget`).

---

**Architectural question — short answer: useful as a *pilot primitive*, not as a *global backbone*.** agent-doc is multi-process; `StateMachine` is single-threaded in-process. Adopting it widely would either buy nothing (for state already persisted to sidecars) or require a missing `ThreadSafeStateMachine`.

**What I found in the codebase:**

| Surface | Today | StateMachine fit |
|---|---|---|
| `agent-doc-orchestration/Cargo.toml` already depends on `lazily = "0.12.0"` | Used in `graph.rs` as a **dependency-graph cache** (Context + CellHandle<PathBuf> + SlotHandle for `project_root`, `snapshot_path`, `doc_hash`, etc.) | None — graph.rs models data deps, not FSMs. No state transitions. |
| `CyclePhase` (Idle → PreflightStarted → ResponseCaptured → WriteApplied → Committed, plus Interrupted(FaultPoint)) | `sim_world.rs:20` enumerates it as an enum + match; production `cycle_state.rs` persists phase to a JSON sidecar. **But within a single `RunContext` the phase is threaded manually.** | **High.** RunContext already owns a lazily::Context. StateMachine<CyclePhase, CycleEvent> gives free `on_transition` logging, `state_is(WriteApplied)` reactive guards, and `ctx.batch` atomicity for multi-step transitions. The in-process portion of cycle state fits cleanly. |
| `SupervisorLifecycle` (Starting → Ready → Busy → WaitingInput → Blocked → Closed) | Enum + match in `sim_world.rs:269`; production state lives in tmux capture + supervisor IPC + JSON sidecar | **Low (production).** The authoritative state is cross-process (tmux pane + supervisor actor), not an in-process cell. Context is RefCell/single-threaded — can't back a supervisor that spans panes. Would need ThreadSafeStateMachine, which lazily does not ship (`state_machine.rs` only frees Context-bound). |
| `QueueControlState` (Resumed / Paused / Draining) | Enum + match; persisted in document frontmatter + sidecar | **Low.** Same reason — authority is the document, not in-process. |
| Pane route / IPC transport / drain-lease states | Ad-hoc match + ops.log markers | **Low.** These states' transitions are observed via tmux capture / IPC acks, not driven from a cell. |

**What StateMachine would buy agent-doc if piloted on CyclePhase:**

1. **Reactive `state_is(...)` signals** — replace the ad-hoc "did we cross WriteApplied?" boolean plumbing in `run.rs`/`write.rs` with `ctx.signal(|ctx| state_handle.get(ctx) == CyclePhase::WriteApplied)`.
2. **`on_transition(|old, new| ...)`** — uniform phase-transition logging + ops.log markers, replacing per-call-site log statements.
3. **PartialEq no-op suppression** — a Starting→Starting re-emission doesn't fire spurious dependents. Today this is hand-coded.
4. **`ctx.batch` atomicity** — multi-step phase transitions (e.g. WriteApplied→Committed + boundary reposition) settle before observers fire, which is exactly the closeout ordering hazard that `#postcommit-ipc-worktree-corruption` just exhibited.

**What blocks broader adoption:**

- **ThreadSafeStateMachine doesn't exist.** `state_machine.rs` only binds to single-threaded Context. The watch daemon / supervisor / project-control-pane are long-lived and need Send + Sync. Until that lands, only short-lived RunContext-shaped surfaces can adopt StateMachine.
- **Most agent-doc state authority is not in-process.** A reactive cell graph only helps when the cell is the authority. For supervisor / queue / pane state, the JSON sidecar / tmux capture / IPC acks are the authority — wrapping an in-process StateMachine around a shadow of those would create a second source of truth and regress the very clobber hazards `realtime_model.rs` exists to prevent.
- **No reactive consumers in production today.** `graph.rs` uses lazily for caching, not for state-derived computation. Adopting StateMachine without also adding `ctx.signal`/`ctx.memo` downstream consumers is a no-op over the existing enums.

**Recommendation:**

1. **Pilot, don't backbone.** Use `StateMachine<CyclePhase, CycleEvent>` inside RunContext only — that's the one surface that is in-process, already has a Context, and currently threads phase manually. Logged as `#lzadocsm` in the backlog.
2. **Don't** adopt it for supervisor / pane / queue / IPC transport — those are cross-process and would create dual-authority hazards.
3. If the pilot proves the reactive-transition pattern, the next enabling step in lazily would be ThreadSafeStateMachine (mirror `state_machine.rs` on ThreadSafeContext) — but that's a lazily-rs feature, not an agent-doc decision, and only unlocks the watch-daemon in-process cache (still not the cross-process surfaces).

Queue drained. No code changes this cycle (the rename is already shipped; the architectural answer is "pilot, deferred").

### Re: ThreadSafeStateMachine + AsyncStateMachine shipped — glm-5.2

> **Queue prompt:** Add ThreadSafeStateMachine and AsyncStateMachine (Tokio).

Executed the full `#spec-test-build-install-commit-push` cycle. Both primitives mirror the single-threaded `StateMachine<S, E>` design — a `CellHandle<S>` as current state + a pure `Fn(&S, &E) -> Option<S>` transition function — over their respective multi-context backends.

**What shipped** (`src/state_machine.rs`, +~370 lines):

| Primitive | Backend | State handle | `send`/`state` | `on_transition` | `state_is` | Gate |
|---|---|---|---|---|---|---|
| `ThreadSafeStateMachine<S, E>` | `ThreadSafeContext` (lock-backed) | `CellHandle<S>` (shared id) | sync | `-> EffectHandle` (dispose via `ctx.dispose_effect`) | `-> ThreadSafeSignalHandle<bool>` | always available |
| `AsyncStateMachine<S, E>` | `AsyncContext` (Tokio) | `AsyncCellHandle<S>` | sync (cells are the sync input layer) | `-> AsyncEffectHandle` | `-> AsyncSignalHandle<bool>` | `#[cfg(feature = "async")]` |

Both require `S: PartialEq + Clone + Send + Sync + 'static` and `E: Send + Sync + 'static`; the transition fn is `Arc<dyn Fn(&S, &E) -> Option<S> + Send + Sync>`. Both implement `Clone` (clone = another handle to the same state cell + transition fn), which is what lets a `ThreadSafeStateMachine` be shared across OS threads.

**Design notes:**
- `ThreadSafeStateMachine` observers (`on_transition`/`state_is`) fire **synchronously** within the invalidating `send`/`batch` call — preserving `ThreadSafeContext`'s glitch-free pull-based ordering. Previous-state tracking in `on_transition` uses `parking_lot::Mutex<Option<S>>` (the crate's existing non-optional dep) instead of the single-threaded `RefCell`.
- `AsyncStateMachine.send`/`state` are synchronous by design: `AsyncContext` cells are the synchronous input layer. Only derived slots/effects/signals are async. `on_transition`'s handler closure is sync `Fn(&S, &S)` (it observes the transition, not async resolution); the effect body reads the cell via `AsyncComputeContext::get_cell` which registers the dependency. Eager recomputation settles on the runtime rather than within `send`.
- No new feature gate for the thread-safe variant (`thread_safe` is always compiled; `parking_lot` is a non-optional dep). The async variant rides the existing `async` feature.

**Tests** (22 new):
- `tests/thread_safe_state_machine.rs` — 12 tests mirroring the 11 single-threaded FSM tests + a cross-thread sharing test (`Arc<ThreadSafeContext>` + `m.clone()` across `std::thread::spawn`).
- `tests/async_state_machine.rs` (`#![cfg(feature = "async")]`) — 10 tests using `#[tokio::test]`, settling spawned effects via the crate's established `tokio::time::sleep(50ms)` convention.

**Verification:**
- `cargo fmt --check` — clean.
- `cargo clippy --all-features --all-targets -- -D warnings` — clean.
- `cargo test --all-features` — all suites green, incl. 22 new tests + 4 doc-test compile checks (`StateMachine`, `ThreadSafeStateMachine`, `AsyncStateMachine` + the existing `state_machine::StateMachine` line 41). 0 failures across every target.
- `make check` — green except the known environmental `benchmark-check` (`#lzbenchbudget`: missing Criterion p50/p95 latency rows for `thread_safe_*_contention`/`graph_propagation` benches). Confirmed **pre-existing on clean HEAD** (`git stash` + `make benchmark-check` reproduces identically) — state_machine.rs touches zero benchmarked code, so this is not a regression.
- lazily-rs CI run `28040292195` was `in_progress` at closeout; continuing from local evidence per the closeout rule (not blocking on `gh run watch`).

**Commits + push (executed):**
- lazily-rs submodule `b724817` (`feat: add ThreadSafeStateMachine and AsyncStateMachine`) → `lazily-hub/lazily-rs` main (`2948b78..b724817`).
- superproject `55b1ebc17` (`chore(lazily-rs): bump gitlink`) → `btakita/agent-loop` main (`ce8fe30a5..55b1ebc17`).
- (`cargo build --all-features` is the install-equivalent — lazily is a library crate, no binaries.)

**Unblock note:** the `#lzadocsm` review items (#c7j5 / #w4pq — pilot `StateMachine` on agent-doc's `RunContext::CyclePhase`) were "Blocked on: lazily ships no `ThreadSafeStateMachine` yet." That blocker is now **lifted** — the thread-safe primitive exists. The pilot itself is agent-doc-repo work, separate from this cycle; the two review items are duplicates and could be deduped into one actionable follow-up when that pilot is taken on.

Queue drained.

### Re: re-evaluate StateMachine as agent-doc backbone — still not a backbone; pilot narrower than stated — glm-5.2

> **Queue prompt:** Reevaluate using state machines as the agent-doc backbone.

**Verdict unchanged: not a backbone.** Shipping `ThreadSafeStateMachine`/`AsyncStateMachine` (lazily `b724817`) does not move agent-doc's adoption needle, because the real blocker was never `Send+Sync` — it's that agent-doc's state authority is cross-process (disk sidecar + editor buffer + IPC + tmux), and a reactive cell only helps when the cell *is* the authority. Two source reads this cycle sharpen this beyond the prior turn.

**1. `CyclePhase` is sidecar-persisted, not `RunContext`-held.** The prior turn claimed "RunContext already owns a lazily::Context … threads phase manually." Half right. `graph.rs:222` does cache `CycleState` in a `RunContext` lazily slot (read-side cache), but the **authority** is the JSON sidecar: every transition in `cycle_state.rs` is `load() → mutate field → save()` (`mark_write_applied:531`, `mark_committed:1041`), with `cycle_phase_rank` (`:1273`) guarding backwards moves. The cell is a shadow; the sidecar is the truth across processes/restarts.

**2. The authority model is multi-source by design.** `realtime_model.rs:115` `reconcile_current_doc` picks `Disk` vs `EditorBuffer` every read; `broadcast_editor_change` (`:447`) merges across N open editors via CRDT. An in-process cell can't be authoritative against that — it would be a third source of truth, regressing the clobber class `realtime_model.rs` exists to prevent.

**Why `ThreadSafeStateMachine` doesn't change it:**
- The in-process pilot surface (`RunContext`, single CLI run) needs only single-threaded `StateMachine` — which already existed. `ThreadSafeStateMachine` targets long-lived `ActorContext` (watch daemon/supervisor, `graph.rs:627`), and those surfaces observe tmux/IPC; they don't drive state from a cell. No FSM semantics today.
- agent-doc-orchestration pins `lazily = "0.12.0"` from crates.io with **no workspace path-patch** to the submodule. `ThreadSafeStateMachine` is on lazily-hub `main` (`b724817`) but not in the published `0.12.0` agent-doc consumes — so "blocker lifted" is premature for agent-doc's actual dependency until a `>0.12.0` release ships.

**What a viable pilot would actually require (narrower than `[recommended]`):**
1. `StateMachine<CyclePhase, CycleEvent>` in `RunContext`, seeded from the sidecar at start.
2. `on_transition` handler that persists to the sidecar (the cross-process authority) — the cell is in-process-authoritative only for the duration of one CLI run.
3. A reactive consumer (`state_is(WriteApplied)` guard, a memo) replacing existing boolean plumbing — the only concrete win.
- **PCP-side:** an optional cache/projection of phase in `ControllerMemoryState` for routing decisions, analogous to the `RunContext` read-side cache at `graph.rs:222` — a shadow, not the truth.
2. **Lost-update** — `flock(2)` advisory lock around `load→mutate→save`, or a CAS on mtime/etag. Larger change; since phase itself is already protected by `cycle_phase_rank`, the remaining exposure is non-phase bookkeeping, so this is hardening rather than a live fire.
1. **Atomic write** — `tempfile::NamedTempFile::new_in(parent)` → write → `persist(path)` (rename is atomic on POSIX same-FS). `tempfile` is already a dev-dep. **Eliminates torn reads entirely**; low-risk, clearly correct.
- **In-process:** `StateMachine<CyclePhase, CycleEvent>` in `RunContext`, seeded from the sidecar at start, `on_transition` → sidecar save, with `state_is(WriteApplied)` reactive guards. Single-threaded `Context` suffices. This is the #c7j5 pilot — a small ergonomic win, not an architecture change.

That's a small ergonomic improvement on one in-process slice, not a backbone. Calling it `[recommended]` overstated it; corrected the two duplicate `#lzadocsm` review items below.

**`#spec-test-build-install-commit-push`: no code work this cycle.** The re-evaluation concludes "no" — no spec/test/build/install changes to lazily-rs. The session-doc commit is the only commit; nothing to push in lazily-rs.

**Bookkeeping:** resolved the verbatim-duplicate review item `#w4pq`; edited `#c7j5` to drop the stale "blocked on ThreadSafeStateMachine" framing and reflect the corrected, narrow scope.

Queue drained.

### Re: CyclePhase authority — JSON sidecar vs Project Control Pane — glm-5.2

> **Queue prompt:**
>
> Does the Sidecar <-> PCP IPC create race conditions?


> **Queue prompt:**
> re [#c7j5]: Why is CyclePhase authority a JSON sidecar? Can it be placed into the Project Control Pane process?

**Why it's a sidecar.** `CyclePhase` is touched by a sequence of independent short-lived CLI processes: `agent-doc preflight` marks `PreflightStarted`, the harness runs as its own process, `agent-doc finalize`/`write --commit` marks `WriteApplied`/`Committed`, and the codex `Stop` hook, `git::commit`, `capture`, `repair`, and `session-check` paths each call the `mark_*` transitions from their own process. The callers of `cycle_state.rs`'s `mark_*` span `run.rs`, `write/run_entry.rs`, `preflight.rs`, `git.rs`, `codex_hook.rs`, `capture.rs`, `repair.rs`, `session_check.rs`, and `flow/closeout.rs`. No single process owns the cycle end-to-end, so the phase must live in a durable record each process can `load() → mutate → save()` (`cycle_state.rs:1186`), with `cycle_phase_rank` (`:1273`) enforcing monotonic forward progress across those writers. The sidecar is the lowest-common-denominator coordination point that survives crashes and needs no other process alive.

**Why not the Project Control Pane.** The PCP (`agent-doc controller serve … --launch-mode lazy`) is a *lazily*-launched routing/supervision process — it owns pane bindings, session actors, dispatch authorization, and the launch lock (`project_controller.rs`), not cycle execution. Crucially, `cycle_state.rs` has **zero** coupling to it (no controller/socket references at all): the cycle authority is deliberately controller-independent so closeout works whether or not the PCP is up. Moving the authority into the PCP would (1) make every transition block on a lazily-booted controller socket, (2) make the PCP a single point of failure for cycle integrity — exactly the crash-recovery case `#durablerecycle`/interrupted-cycle detection needs to survive, and (3) still require an on-disk record anyway, because the editor plugin reads the phase without IPC (`cycle_state.rs:305`/`:1072`) and recovery must work when the controller was the thing that crashed.

**What is viable (the #c7j5 narrow win).** Keep the sidecar as the durable authority; layer shadows on top:

- **In-process:** `StateMachine<CyclePhase, CycleEvent>` in `RunContext`, seeded from the sidecar at start, `on_transition` → sidecar save, with `state_is(WriteApplied)` reactive guards. Single-threaded `Context` suffices. This is the #c7j5 pilot — a small ergonomic win, not an architecture change.
- **PCP-side:** an optional cache/projection of phase in `ControllerMemoryState` for routing decisions, analogous to the `RunContext` read-side cache at `graph.rs:222` — a shadow, not the truth.

Promoting the PCP to the write authority only pays off if the PCP were ever made mandatory/always-on; today that trades a local atomic file write for a socket round-trip on every transition without removing the on-disk record, so it is not a clear win. The #c7j5 re-scope stands: sidecar stays the authority, the `StateMachine` pilot stays narrow.

### Re: CyclePhase visibility in the PCP — already visible at closeout; a live projection aids recovery, not prevention — glm-5.2

> **Queue prompt:** Would it benefit the agent-doc lifecycle if the CyclePhase were visible in the PCP? Would this visibility help to fix issues chronic to agent-doc? The PCP/Supervisor are running from `agent-doc start`.

**Already visible — at closeout.** The PCP reads `CyclePhase` today: `persist_session_actor_closeout` calls `cycle_state::load(file)` (`project_controller.rs:1489`) and projects phase + queue head + pending ids into its SQLite state store (`:1511`). So the real question is whether a *live, always-on* projection (vs a closeout-only read) would help the chronic issues.

**It helps recovery/observability, not prevention.** The chronic issues — as this session itself exhibited (a captured response lost to a wedged-write repair, `live_prompt_drift_after_preflight`, repeated no-op closeouts) — are not caused by the supervisor *not seeing* the phase. They're caused by:

1. **Non-atomic sidecar writes** → torn reads by any reader incl. the PCP. **Shipped this cycle** (`#lzsidecaratomic`: `cycle_state::save` now temp-file + atomic `persist`; agent-doc `647af082`; +regression test).
2. **Capture-retirement policy** — a write that fails IPC proof is classified "wedged" and the captured body is retired for forensics, so the response vanishes from the doc. This is what ate the prior Sidecar-race answer — its conclusions survive only because they're restated below.
3. **IPC proof brittleness** (`live_prompt_drift_after_preflight`) — the editor buffer's prompt position doesn't match the disk baseline, so delivery can't be proven even when the socket ACKs.

A live phase projection in the PCP lets the supervisor *notice* a cycle stuck at `response_captured` and trigger recovery sooner — a real latency win. It does nothing for (1)–(3); those need the write/capture/IPC paths hardened. Worth adding as a *shadow* (seed `ControllerMemoryState` from the sidecar, the authority) — **not** worth promoting to the write authority (the prior turn's verdict stands: the PCP is deliberately cycle-independent so closeout survives a controller crash).

**`do [#lzsidecaratomic]`: done (atomic-write half).** `cycle_state::save` → `write_atomic()` (stage `NamedTempFile` in the parent dir → write → `persist`, i.e. atomic `rename` on POSIX same-FS). Every writer (`mark_*`, `record_*`, `start_preflight`, ~42 call sites) becomes atomic for free, eliminating torn reads. 39/39 `cycle_state` tests green incl. new `save_is_atomic_leaves_no_temp_residue`; `cycle_state.rs` clippy-clean (the 40 pre-existing errors are in `sync.rs`/`write/ipc.rs` on Rust 1.96, untouched). Shipped: agent-doc `647af082` → `btakita/agent-doc` main; superproject gitlink `af2db96e5`.

**Deferred (optional half → `#lzsidecarlock`):** the *lost-update* window among concurrent writers (load→mutate→save with no lock/CAS) — `cycle_phase_rank` still guards only monotonic *phase*, not the ~30 non-phase fields. Needs a flock/CAS refactor across the call sites (deadlock-prone, design-heavy).

**Deploy note:** `make install-full` + `agent-doc admin recycle` run *after* this closeout so the binary recycle can't destabilize the session closeout itself (6 no-op closeouts this session — not risking another).

*Restated (the prior Sidecar↔PCP answer was lost to the wedged-capture closeout):* no Sidecar↔PCP IPC exists — they share only the filesystem one-way; the PCP reads the sidecar at `:1489` and never writes it (writes go to its own SQLite store). The real race is among the ~42 sidecar writers via non-atomic save — now fixed for the torn-read class; the lost-update class remains (`#lzsidecarlock`).

Queue drained.
<!-- agent:boundary:c78fe419 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~Does the Sidecar <-> PCP IPC create race conditions?~~
- ~~Would it benefit the agent-doc lifecycle if the CyclePhase were visible in the PCP? Would this visibility help to fix issues chronic to agent-doc? The PCP/Supervisor are running from `agent-doc start`.~~
- ~~do [#lzsidecaratomic]~~
- do [#lzsidecarlock]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzsidecarlock] [recommended] agent-doc-repo: close the lost-update window among concurrent cycle_state writers (load->mutate->save across ~42 call sites has no lock/CAS). cycle_phase_rank (:1273) guards only monotonic phase, not the ~30 non-phase fields (pending_done_ids, active_queue_heads, capture_id, file_hash...). Approach: advisory flock (fs2 is already a prod dep) or CAS on mtime/etag around the load->mutate->save window — deadlock-prone across many call sites, so centralize the pattern. Split from #lzsidecaratomic (atomic-write/torn-read half is DONE in agent-doc 647af082).
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
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
