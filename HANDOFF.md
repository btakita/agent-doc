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

## Log

- (start) HANDOFF created.

## Remaining items

(filled in as work proceeds)
