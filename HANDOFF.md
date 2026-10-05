# HANDOFF — netadv2 (shared adversarial channel module)

Item: #netadv2 (plan: agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md, "Message loss" + phase 2)
Branch: netadv2 (worktree /home/brian/work/btakita/agent-doc-wt/netadv2). Do NOT push/install/release/merge.

## Goal
formal/tla/NetChannel.tla (bag channel: Send, Deliver, Drop, Duplicate, Reconnect; fair-lossy fairness op),
reference protocol formal/tla/NetChannelRetransmit.tla (resend-until-ack, idempotent receiver keyed by seq+gen),
Wedge cfgs: (a) fire-and-forget violates liveness, (b) non-idempotent receiver violates exactly-once,
(c) no generation check applies a stale message. Wire into scripts/run_tla.sh must_violate; document in formal/tla/README.md.

## Done
1. formal/tla/NetChannel.tla — library (INSTANCE'd, not in modules list). Vars net (bag), gen, delivered.
   API: ChannelInit, ChannelTypeOK(Msgs), Send(m), Deliver(m), DeliverAndSend(m,r), Drop, Duplicate,
   Reconnect, Adversary, InFlight, chanVars, FairLossy(Msgs) (= \A m: SF_chanVars(Delivered(m, Msgs))).
2. formal/tla/NetChannelRetransmit.tla + .cfg (positive), Reach, FireAndForgetWedge, DuplicateWedge,
   StaleGenWedge, UnfairWedge cfgs.
3. scripts/run_tla.sh: NetChannelRetransmit in modules, new libraries=(NetChannel) copied, 5 must_violate entries.
4. formal/tla/README.md: "Adversarial network: NetChannel" section.
5. `make tla` EXIT=0: 25 modules pass, all must_violate confirmed incl. 5 NetChannelRetransmit ones.
   Positive run 32,443 distinct states, ~6s.

## Findings
- TLC 1.7.4 cannot check a temporal formula built from an operator ARGUMENT (FairLossy(Msgs, Recv(_), v) ->
  "TLC cannot handle the temporal formula"), even without INSTANCE. Hence the `delivered` marker variable,
  so the channel itself names the receive step; Drop sets delivered' = {} so loss never counts as delivery.
- SF action for fairness must pin every channel var (TLC computes ENABLED by generating successors), hence
  Delivered(m, Msgs) enumerates net' over {Take} \cup {Put(Take, r) : r \in Msgs}.
- WF is insufficient for fair-lossy: Drop interrupts enabledness between resends, so SF per message.

## Remaining
None (final commit on branch netadv2; not pushed/merged/installed).
