# HANDOFF — netadv6 (deterministic simulation fuzzing)

Item: `#netadv6` from `tasks/agent-doc/plan-network-adversarial-correctness.md`.
Branch `netadv6`, worktree `agent-doc-wt/netadv6`. Do NOT push, install,
release, merge to main, version, tag, or publish.

## Design

- `agent-doc-sim-net`: `SendPlan` records per-send copy offsets and stalls;
  `send_recorded` draws and returns a plan, while `send_planned` replays it
  without RNG. `recorded_plans_replay_the_same_deliveries` protects identity.
- `src/sim_world/fuzz.rs`: deterministic explorer over the same
  `SimWorld::apply` engine and adversarial `SimNet` routing. `FuzzTrace` contains
  the network profile plus command/plan steps and ticks, round-trips as text,
  and shrinks to a fixed point without inventing unreachable schedules.
- Oracle hooks surround every local command and delivered message. Oracles
  mirror the TLA+ properties for unique ownership, operator-text retention,
  exactly-once response commit, dispatch safety, generation fencing, bounded
  stale layout, and eventual recycle consumption.
- Regression schedules live in `src/sim_world/fuzz_seeds.txt` and run in
  `sim_fuzz_regression_seeds_replay`. The fixed short budget exercises 400
  seeds × 80 steps and enforces coverage floors. `make sim-fuzz` supplies the
  configurable ignored long-budget runner; the nightly workflow uploads traces.

## Findings fixed by netadv6

1. Delayed readiness could promote a successor generation. The model now uses
   the sender's observed generation.
2. Route dispatch and idle draining could cross a recycle-inflight boundary.
   Both are deferred until settlement.
3. A dispatch proof for generation N could prove a generation N+1 receipt.
   Proof and receipt generations must match.
4. Duplicate-response generation could invent an impossible capture; the step
   now requires the duplicated block to be the last visible response.
5. Snapshot-save recovery restored the live document rather than the committed
   head. Recovery now restores `interrupted_commit_head`.
6. Recycle liveness once survived a bare stale-marker transition; consuming the
   request now resets that oracle obligation.

## Reconciliation with netadv3/netadv5/GH #136

Commit `2a492d030` was merged after the netadv6 milestones. It includes
`dc17e89d3`, whose send-stamp fence orders lifecycle, heartbeat, and queue
control updates against both newer messages and controller-local transitions,
plus netadv5's durable dispatch idempotency key and the completed GH #136 stale
layout/recycle behavior.

The reconciled simulator now mirrors the production stamp source instead of
using a network request id as both concepts: level updates carry a separate
per-host IO-clock stamp, retransmits retain that stamp, and receiver-local
lifecycle call edges (including admin reap and session restart transitions)
advance the same order. The deterministic delayed-Ready/local-Dead regression
protects this boundary. A rejected no-op delivery is not reported as an applied
out-of-order mutation.

The previously open
`stale_actor_lifecycle_overwrote_local_transition` schedule is therefore a
clean regression. The older SIM-F1, SIM-F2, SIM-F3, stale-layout widening, and
recycle-lapse schedules are clean regressions too. `KNOWN_FUZZ_FINDINGS` is
empty; a recurrence is a test failure rather than an allow-listed result.

## Historical budgets

- 15 minutes, base seed 1,000,000, 160 steps: 42,216 schedules. It found the
  snapshot-save recovery defect above; all other observed kinds were then-known
  sibling/GH #136 findings.
- 5 minutes, base seed 7,000,000 after that fix: 13,052 schedules and no new
  finding kinds.

## Final verification

- `sim_fuzz_regression_seeds_replay`: 1 passed; all seven recorded schedules
  replay clean.
- Deterministic delayed Ready vs local Dead regression: 1 passed.
- netadv4 F1/F2/F3 deterministic regressions: 3 passed.
- Fixed short simulation-fuzz budget: seeds 0..400, 400 schedules, 41,329
  structural checks, no finding kinds.
- Fresh long budget: base seed 9,000,000, 2,395 schedules × 160 steps in 30
  seconds, no finding kinds.
- Full `make check` with explicit exit status and native failure tally: pending.
