# netadv3 handoff

Task `#netadv3` (plan: `tasks/agent-doc/plan-network-adversarial-correctness.md`
in agent-loop). Branch `netadv3`, based on `batch-0505` (gh136 + netadv1 + netadv2
merged). Do NOT push, install, release or merge.

Scope: re-check the hot-path TLA+ models over `formal/tla/NetChannel.tla` and fix
LOST-MESSAGE / one-shot defects (audit `docs/reference/network-channel-audit.md`,
F1-F23). Out of scope (sibling `netadv5`): timeout-as-verdict R1, R2, R3, R5, R7,
R8, R9 (F1, F2, F3, F6, F19, F20) and round-trip budget tests.

## Status

- [ ] 1. Port StaleColumnRecycle's plain-set channel onto NetChannel
- [ ] 2. Net models: VisibleDeliveryReceipt, EditorReplicaStrand, RecycleSettleDispatch, AgentDocCloseout, PassiveTmuxSync
- [ ] 3. Code fixes for counterexamples (+ regression + Wedge cfg each)
- [ ] 4. `make tla` + `make check` green, exit status captured

## Plan / ids

Code-defect ids used here: audit F1-F23/R1-R12; netadv4 SimWorld findings
SIM-F1 (lifecycle/heartbeat last-arrival-wins), SIM-F2 (queue control
last-arrival-wins); new ones found by these models: ERS-1, RSD-1, VDRN-post-ack.

Models (formal/tla, run by scripts/run_tla.sh):
- StaleColumnRecycle: ported onto NetChannel (in place).
- VisibleDeliveryReceiptNet: wedges F9 (WakeOneShot), F10 (RecoveryLatch),
  F13/F12 (SaveOneShot), StaleAck (receipt keying), TimeoutRefusal (R2, netadv5).
- EditorReplicaStrandNet: wedges ERS-1 (Latch), GiveUpRefusal (R2/R3, netadv5).
- TODO: RecycleSettleDispatchNet (RSD-1 unreachable-controller read as refusal;
  R9 TTL-as-proof wedge, netadv5), AgentDocCloseoutNet (F4/F5 lease),
  PassiveTmuxSyncNet (F14/F15 focus graph advances before effect),
  LifecycleSequence (SIM-F1/F2 seq fence).

Code fixes planned (in order): F10, F9, F13, ERS-1, RSD-1, F4/F5, SIM-F1/F2
(merge netadv4 first), then F11, F16, F17, F18, F14/F15, F21 if time allows.

## Log

- (start) HANDOFF created.
- StaleColumnRecycle ported (positive 481k states, 40s); all 7 wedge/reach cfgs violate as required.
- VisibleDeliveryReceiptNet + 8 cfgs; TLC: positive + safety pass, all wedges/reach violate.
- EditorReplicaStrandNet + 4 cfgs; TLC: positive passes; Latch wedge trace = budget whose
  receipts were all dropped -> Exhaust latches -> stuck (ERS-1).

## Remaining items

(filled in as work proceeds)
