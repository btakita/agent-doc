# HANDOFF — netadv4 (SimWorld network fault injection)

Item: `#netadv4` from `agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md`
(read its "Message loss" section). Branch `netadv4`, worktree `agent-doc-wt/netadv4`.
Do NOT push, install, release or merge.

## Goal
Seeded, deterministic network-conditions layer for SimWorld: `NetConditions`
(latency min/max, jitter, reorder, drop, duplicate, reconnect, half-open stall),
profiles `local` (zero faults, existing tests byte-identical), `coder_zscaler`, `hostile`.
Route SimWorld's cross-process messages through it, run existing scenarios under
coder_zscaler + hostile across seeds, record failures (seed + minimal trace), add a
make hook so `make check` runs a fixed seed set under coder_zscaler.

## Done
1. New pure crate `agent-doc-sim-net` (NetConditions, LatencyModel, NetProfile
   local/coder_zscaler/hostile, SimNet<L,M> with Delivery::{AtLeastOnce,FireAndForget},
   seeded splitmix RNG, trace). 7 unit tests green.
2. `src/sim_world/net.rs`: SimLink classification, generation stamped at send,
   Rpc mode (scripted tests, env `AGENT_DOC_SIM_NET_PROFILE`/`AGENT_DOC_SIM_NET_SEED`)
   and Async mode (corpus, `run_seed_with_net`, `net::run_net_corpus`). Oracles:
   stale level-update reorder, dispatch accepted on reordered stale lifecycle,
   duplicate dispatch injected twice. Panic dumps net trace tail.
3. `engine.rs`: `apply` -> net wrapper; `apply_local` = old body; generation reads
   in networked arms go through `observed_generation()`; `new_local`.

4. Tests: `src/sim_world/net.rs` tests (local byte-identical, seed reproduces,
   corpus coder_zscaler/hostile [ignored, run by `make sim-net`], known-defect wedge
   tests F1/F2/F3, cross-generation straggler rejected). Ratchet
   `KNOWN_OPEN_NET_FINDINGS`.
5. Install-fanout SimWorld (rpc.rs `install_fanout_idle_root_tests`) routes status +
   reload through SimNet (FireAndForget, 2s timeout); new test
   `install_fanout_under_adversarial_net_stays_safe_and_accounts_every_endpoint` green.
6. Makefile `sim-net` target, added to `check`. `make sim-net` exit 0.

## Remaining (superseded list below kept for history)
1. Add pure crate `agent-doc-sim-net` (publish=false, version = workspace version,
   add to root `[workspace] members` + dev-dep of root crate and agent-doc-controller-io;
   update Cargo.lock). Holds `NetConditions`, profiles, `SimNet<M>` with its own
   seeded RNG (separate from the schedule RNG so `local` keeps schedules identical).
2. Main SimWorld: `src/sim_world.rs` (struct ~L5969, `DeterministicRng` ~L5994,
   corpus tests ~L6318/L6449) and `src/sim_world/engine.rs` (`run_seed` L42,
   `run_seed_corpus` L116, `apply` L135). Classify cross-process SimCommands
   (Supervisor* lifecycle, StaleSupervisorUpdate, heartbeat, DispatchRoutePrompt,
   ProveDispatchAccepted, Admin*, Sync*, PostCommitIpcRepositionSignal,
   ObserveStale/MissingPane, BindRouteOwner) and send them via SimNet. Messages that
   read `route.durable.generation` at apply time must stamp it at SEND time so delay
   produces genuinely stale deliveries. `local` = deliver immediately.
3. Add `run_seed_with_net` / corpus over profiles; tests for coder_zscaler + hostile.
4. Install-fanout SimWorld in `agent-doc-controller-io/src/project_controller/rpc.rs`
   (~L41014): route `status` + reload delivery through SimNet; assert safety
   (never launch idle root; delivered+failed accounting) under faults.
5. Makefile: extend `sim-medium` (L106) or add `sim-net` target, include in `check` (L249).
6. Run `make check` with explicit exit status capture (RTK masks it).
7. Commit; final report.

## Findings so far
Corpus (FAST seeds 0..512 x net seeds 0..4, 24 steps): 0 structural failures in both
profiles; closeout commits identical (272). Oracle findings:
- F1 lifecycle reorder: `LifecycleRequest` (agent-doc-controller-io/src/project_controller.rs:8115)
  and heartbeat carry `generation` only, no per-generation sequence -> older Ready/Busy/
  WaitingInput applied after newer. coder_zscaler 144, hostile 670. Impact: dispatch
  accepted while supervisor last reported busy (coder seed 204/net 0; hostile 7x).
  Scripted: route_sim_repairs_stale_busy_projection_with_ready_prompt_then_dispatches
  (hostile net seed 0) dispatches into Busy; accepted_stale_supervisor_replacement_timeout_preserves_mid_turn_session
  (hostile 1) Busy straggler after Ready -> reexec never happens (liveness);
  restart_supervisor_sim_refuses_alive_busy_without_force_then_force_kills (hostile 1)
  pre-restart Busy straggler overwrites Starting (model restart keeps generation).
- F2 queue-control reorder (pause/resume/drain), coder 32 / hostile 153.
- F3 duplicate dispatch request injected twice after proof (no request id in
  ControllerRequest): hostile seed 8 net_seed 1 (3x).
- Counter-only (safe, idempotent outcome, exact-counter asserts trip): admin handoff/reap
  dup -> extra stale block (coder 2, hostile 2/3/5); qflood coalesce counters; sync
  tmuxbudget dup sync (coder 7); queue_controls dup resume; starting/busy dup dispatch blocks.


## Commands / status
- `make sim-net` -> EXIT=0.
- `make check` pending (next step).
