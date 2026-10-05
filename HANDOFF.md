# HANDOFF — netadv6 (deterministic simulation fuzzing)

Item: `#netadv6` from `agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md`.
Branch `netadv6`, worktree `agent-doc-wt/netadv6`, based on `netadv4` (agent-doc-sim-net
crate, `src/sim_world/net.rs`, `make sim-net`). Do NOT push, install, release or merge.

## Goal
Seeded explorer over SimWorld (interleavings, SimNet faults, operator edits,
recycles/installs, crashes, reconnects) + TLA-mirroring oracles after every step +
shrinking to a replayable trace + regression seeds file replayed by `make check` +
`make sim-fuzz FUZZ_SECS=...` + nightly workflow. Known netadv4 defects F1/F2/F3 are
allow-listed by finding kind; report NEW kinds.

## Plan
1. agent-doc-sim-net: explicit per-send `SendPlan` (record on generation, replay on
   shrink) so a trace replays without the channel RNG.
2. `src/sim_world/fuzz.rs`: palette, `FuzzTrace` text form, oracles, shrinker,
   budgets, seeds file `src/sim_world/fuzz_seeds.txt`.
3. Makefile `sim-fuzz`, short budget + seeds in `make check`; nightly workflow.
4. Run 10-20 min budget; triage new findings.

## Status
Milestone 0: started.
