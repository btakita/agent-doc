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
1. Surveyed code (no edits yet).

## Remaining
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
None yet.

## Commands / status
- `make check` not yet run.
