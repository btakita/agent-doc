# HANDOFF — netadv2 (shared adversarial channel module)

Item: #netadv2 (plan: agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md, "Message loss" + phase 2)
Branch: netadv2 (worktree /home/brian/work/btakita/agent-doc-wt/netadv2). Do NOT push/install/release/merge.

## Goal
formal/tla/NetChannel.tla (bag channel: Send, Deliver, Drop, Duplicate, Reconnect; fair-lossy fairness op),
reference protocol formal/tla/NetChannelRetransmit.tla (resend-until-ack, idempotent receiver keyed by seq+gen),
Wedge cfgs: (a) fire-and-forget violates liveness, (b) non-idempotent receiver violates exactly-once,
(c) no generation check applies a stale message. Wire into scripts/run_tla.sh must_violate; document in formal/tla/README.md.

## Done
- Studied conventions: scripts/run_tla.sh (modules list + must_violate Module:Config list; copies only Module.tla/.cfg
  into a temp dir, so a library module like NetChannel.tla must be copied explicitly), README.md, RealtimeSteeringStop*.cfg.

## Remaining
1. Write NetChannel.tla (library, INSTANCE'd; not in modules list) — copy it in run_tla.sh.
2. Write NetChannelRetransmit.tla + .cfg, Reach cfg, 3 Wedge cfgs.
3. Add to run_tla.sh modules + must_violate.
4. README section.
5. Run `make tla` (downloads tla2tools 1.7.4 into target/tla), commit.

## Status
Checkpoint commit only; nothing modeled yet.
