# HANDOFF — netadv6 (deterministic simulation fuzzing)

Item: `#netadv6` from `agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md`.
Branch `netadv6`, worktree `agent-doc-wt/netadv6`, based on `netadv4` (agent-doc-sim-net
crate, `src/sim_world/net.rs`, `make sim-net`). Do NOT push, install, release or merge.

## Design (done)
- `agent-doc-sim-net`: `SendPlan` (per-send copy offsets + stall), `send_recorded`
  (RNG draws, returns plan; `send` = this, byte-identical) and `send_planned` (replay,
  no RNG). Test `recorded_plans_replay_the_same_deliveries`.
- `src/sim_world/fuzz.rs`: explorer over the SAME `SimWorld::apply` engine + netadv4
  SimNet routing (Async mode). `FuzzTrace` = profile + steps (`step <SimCommand> @plan`,
  `tick n`); text form round-trips. `FuzzTrace::generate(seed)` draws from a weighted
  palette (~90 commands: edits, closeout, crashes, restarts, supervisor reports,
  dispatch, tmux observations, layout sync, admin, install/recycle) plus protocol
  FRAGMENTS (closeout cycle, routed turn, install->boundary, rebind) and idle ticks.
  Profile = seed % 3 (local / coder_zscaler / hostile). `step_enabled` preconditions
  make impossible steps no-ops in generation AND replay (so shrinking cannot invent
  unreachable schedules).
- Oracle hooks: `SimWorld.fuzz: Option<Box<FuzzState>>` (None outside fuzz);
  `fuzz_pre/fuzz_post` around every local step (engine `apply`) and every delivered
  message (`net::apply_delivered`); `fuzz_on_send` stamps logical send order.
- Oracles (each names its TLA mirror, `Oracle::tla`):
  UniqueOwner, NoLostOperatorText, ExactlyOnceResponseCommit,
  NoDispatchIntoBusySupervisor (+ recycle boundary), NoStaleGenerationApply,
  StaleStashNeverWidens (GH #136), RecycleEventuallyConsumed (liveness, after a
  deterministic fairness suffix), plus netadv4 NetChannel findings and Structural.
- GH #136 sub-model (fuzz-only SimCommands `InstallFanout`, `SyncFocusStaleStashPane`,
  `AdvanceWallClock`): base production `plan_column_admissions` +
  `recycle_request_is_live`; a live request feeds `supervisor_recycle_action` as
  explicit_admin (`fuzz_recycle_request_live`, false outside fuzz).
- Shrinker: ddmin over steps, then plan->clean, tick->1, profile->local, to fixpoint.
- Regression seeds: `src/sim_world/fuzz_seeds.txt` (`expect clean` / `expect known K`),
  replayed by `sim_fuzz_regression_seeds_replay` in `make test` (so `make check`).
- Budgets: `sim_fuzz_short_fixed_budget_finds_no_new_finding_kinds` (seeds 0..400 x 80
  steps, ~3s, non-vacuity floors) in `make test`; `make sim-fuzz FUZZ_SECS= FUZZ_STEPS=
  FUZZ_SEED= FUZZ_OUT=` (ignored `sim_fuzz_long_budget`); nightly
  `.github/workflows/sim-fuzz-nightly.yml` uploads `sim-fuzz-out/*.trace` on failure.
- Triage helpers (ignored tests): `AGENT_DOC_SIM_FUZZ_SHOW_KIND=<kind>` ->
  `sim_fuzz_show_kind`; `AGENT_DOC_SIM_FUZZ_TRACE=<file>` -> `sim_fuzz_replay_trace_file`.

## Allow-list (`KNOWN_FUZZ_FINDINGS`, keyed by kind)
netadv4 F1/F2/F3 kinds; `stale_actor_lifecycle_overwrote_local_transition` (NEW, F1
variant, see findings); GH #136 `stale_stash_pane_widened_layout` and
`stale_recycle_request_lapsed_unconsumed` (fixed on main 9bb7edb2b, not in this
base: on rebase onto main, drop both, flip their seeds to `expect clean`, and adapt
`fuzz_sync_focus_stale_stash_pane` to main's admission/consumption-bound API).

## Findings so far
NEW, fixed (model fidelity, all `expect clean` seeds):
1. `stale_generation_promote_starting_prompt_ready_mutated_route`: a delayed readiness
   report promoted the NEXT generation (arm read durable generation). Fixed: uses
   `observed_generation()` (prod `LifecycleRequest` is generation-fenced).
2. `dispatch_into_recycling_supervisor`: route dispatch / idle drain injected while
   `recycle_inflight`. Fixed: `dispatch_route_prompt_with` defers while inflight (prod
   route paths wait for settle); fuzz precondition: idle tick cannot run mid-own-execve.
3. `stale_generation_prove_dispatch_accepted_mutated_route`: a dispatch proof observed
   at gen N proved the gen N+1 receipt. Fixed: proof must match receipt generation.
   (netadv4 F3 test now searches the corpus instead of pinning seed 8.)
4. `committed_response_count_exceeds_distinct_captures`: was a generator artifact
   (DuplicateVisibleResponse with no/an earlier visible response); precondition now
   requires the duplicated response to be the last exchange block. An adjacent
   duplicate is never committed (seed guards it).
5. `committed_response_count_exceeds_distinct_captures` (15-min budget, seed 1004404):
   snapshot-save fault recovery copied the CURRENT document (a duplicate response that
   arrived after the commit) into the reviewed baseline. Fixed: the fault records the
   committed content (`interrupted_commit_head`) and recovery restores from it.
6. Oracle bug (not product): liveness flagged a consumed request followed by a bare
   `MarkSupervisorBinaryStale`; consumption now resets the obligation.

NEW, open (allow-listed, needs the sibling F1 fix to cover it):
- `stale_actor_lifecycle_overwrote_local_transition`: a lifecycle/heartbeat report
  sent BEFORE a controller-side transition that keeps the generation (session restart
  -> Starting, socket death -> Dead, admin reap -> Closed) lands after it and resurrects
  Ready; the next dispatch types into a starting/dead pane. Shrunk:
  `profile hostile / step SupervisorReady @904 / step AbandonSupervisorToDeadSocket`.
  A per-generation report sequence alone does not order against controller-local
  transitions; those must bump the generation or advance the fence.

## Budgets run
- 15 min, base seed 1000000, 160 steps: 42216 schedules; one NEW kind (item 5, fixed).
  Known counts: F1 out-of-order 22619, overwrote_local_transition 9199, F1 dispatch
  3469, F3 1583, F2 907, GH136 widen 12585, GH136 lapse 2.
- 5 min, base seed 7000000, after the fix: 13052 schedules, no new kinds.

## Status
Milestones committed. Remaining: `make check` with explicit exit capture.
