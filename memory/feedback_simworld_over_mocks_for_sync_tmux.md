---
name: feedback_simworld_over_mocks_for_sync_tmux
description: "For agent-doc sync/tmux/pane behavior, cover it with deterministic SimWorld tests, not IPC mocks or live-tmux integration tests"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: fb700ab5-3ca8-4cd7-9ad8-2ae501c87121
---

When adding test coverage for agent-doc sync / tmux / pane-reconcile behavior, use the **SimWorld** model (`src/sim_world.rs` + `src/sim_world/engine.rs`), not IPC/supervisor mocks or live-`IsolatedTmux` `#[ignore]` integration tests.

**Why:** Brian's explicit steer ("Why are we using mocks? We should use simworld testing"). SimWorld models pane layout as `SyncProjection { visible, stashed, active, protected_open_cycle }` and reconcile decisions as pure functions, so coverage is deterministic and runs in `make check` — no tmux server, no supervisor-identity/process-tree mocking. Live-tmux tests need a real supervisor (`query_supervisor_identity` + `process_tree_contains_pid`), which pushes you toward mocking the supervisor IPC — exactly what to avoid.

**How to apply:** (1) Extract the decision as a pure `pub fn` in `agent-doc-sync-io::sync` (e.g. `exact_visible_focus_eligible_for_owned_pane_resolve`) so production and the model share ONE rule — mirror the `sync_lock_acquire_decision` (ffi) pattern. (2) Add a `SyncProjection` case + an engine `apply_sync_*` method that calls the shared rule and runs `apply_requested_projection`, plus a `SimCommand` variant. (3) Assert on `world.sync.visible/stashed/active` + coverage counters + `assert_structural_invariants()`. (4) Do NOT wire new commands into the `run_seed` random fuzz — changing `rng.next_usize(N)` reshuffles the whole fixed-seed corpus and breaks its coverage-threshold assertions (`closeout_sim_fixed_seed_corpus_exercises_recent_failure_classes`); keep new deterministic commands in targeted tests only. See AGENTS.md "add or update deterministic SimWorld coverage for the failure class." [[feedback_module_split_functionality_with_tests]]
